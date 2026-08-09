// Kept in the execute module through include! so this structural split does not change visibility or code generation.

pub fn execute(eg: &mut ExecutorGlobals, main_func: &UserFunction) -> Result<Value, VmError> {
    let func_ptr = &main_func.common as *const FunctionCommon;
    let frame = eg.vm_stack.push_call_frame(
        func_ptr,
        0,
        0,
        eg.current_execute_data.get(),
        std::ptr::null_mut(),
    );

    let mut return_value = Value::null();
    unsafe {
        (*frame).return_value = &mut return_value;
        (*frame).opline = main_func.op_array.instructions.as_ptr();
    }
    eg.current_execute_data.set(frame);

    execute_ex(eg, frame)?;

    #[cfg(debug_assertions)]
    super::hot::dump_bail_stats();

    eg.current_execute_data.set(unsafe { (*frame).prev_execute_data });
    unsafe { cleanup_frame_slots(frame) };
    eg.vm_stack.pop_call_frame(frame);

    // Check for uncaught exception that propagated through execute_ex
    if let Some(exc) = eg.exception.take() {
        let (class_name, message) = if let Some(obj) = exc.as_object() {
            let cls = obj.class_name.clone();
            let msg = obj.get_property("message")
                .map(|v| v.echo_to_string())
                .unwrap_or_default();
            (cls, msg)
        } else {
            (std::rc::Rc::from("Exception"), exc.echo_to_string())
        };
        return Err(VmError::Fatal(format!("Uncaught {}: {}", class_name, message)));
    }

    Ok(return_value)
}

/// Call a PHP function by FunctionCommon pointer with given arguments.
/// Used by stdlib functions like array_map/array_filter for callback invocation.
pub fn call_function(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    args: &[Value],
) -> Result<Value, VmError> {
    if unsafe { (*func_ptr).fn_type } == FunctionType::Internal {
        let internal = unsafe {
            &*(func_ptr as *const super::function::InternalFunction)
        };
        if let Some(handler) = internal.direct_handler {
            let common = &internal.common;
            let arity_ok = args.len() >= common.sig.required_num_args as usize
                && (common.sig.is_variadic || args.len() <= common.sig.public_arity() as usize);
            if arity_ok {
                return handler(args);
            }
        }
    }
    call_function_iter(eg, func_ptr, args.len(), args.iter())
}

/// Evaluate a compiler-proven pure Long callback without allocating a VM
/// frame. Callback consumers have already resolved the callable, so this
/// boundary only guards the exact user function, arity and by-value Long ABI.
/// A failed type or checked-arithmetic guard is side-effect free and leaves the
/// caller free to replay the invocation through the canonical PHP path.
#[inline(always)]
pub(crate) unsafe fn try_execute_scalar_long_callback<'a, I>(
    func_ptr: *const FunctionCommon,
    public_num_args: usize,
    arguments: I,
) -> Option<i64>
where
    I: IntoIterator<Item = &'a Value>,
{
    if func_ptr.is_null() {
        return None;
    }
    let common = &*func_ptr;
    if common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || common.sig.public_arity() as usize != public_num_args
    {
        return None;
    }

    let user = &*(func_ptr as *const UserFunction);
    let plan = user.scalar_long_plan.as_deref()?;
    if plan.public_args as usize != public_num_args {
        return None;
    }

    let mut scalar_arguments = [0i64; 8];
    let mut arguments = arguments.into_iter();
    for destination in scalar_arguments.iter_mut().take(public_num_args) {
        let value = arguments.next()?;
        if value.value_type() != ValueType::Long || value.is_reference() {
            return None;
        }
        *destination = value.raw_long();
    }
    if arguments.next().is_some() {
        return None;
    }

    let result = evaluate_scalar_long_plan(plan, &scalar_arguments)?;
    record_scalar_call(common);
    Some(result)
}

/// Prepared exact user callback and pure Long plan. The pointers remain stable
/// for the lifetime of one compiled request; callers still validate every
/// runtime argument before plan evaluation.
#[derive(Clone, Copy)]
pub(crate) struct ScalarLongCallback {
    common: *const FunctionCommon,
    plan: *const ScalarLongFunctionPlan,
    public_num_args: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum ScalarLongSortOrder {
    Ascending,
    Descending,
}

/// Guard the invariant callable identity, signature and scalar plan once.
#[inline(always)]
pub(crate) unsafe fn prepare_scalar_long_callback(
    func_ptr: *const FunctionCommon,
    public_num_args: usize,
) -> Option<ScalarLongCallback> {
    if func_ptr.is_null() {
        return None;
    }
    let common = &*func_ptr;
    if common.fn_type != FunctionType::User
        || !common.supports_scalar_long_plan()
        || common.sig.public_arity() as usize != public_num_args
    {
        return None;
    }

    let user = &*(func_ptr as *const UserFunction);
    let plan = user.scalar_long_plan.as_deref()?;
    if plan.public_args as usize != public_num_args || public_num_args > 8 {
        return None;
    }

    Some(ScalarLongCallback {
        common: func_ptr,
        plan,
        public_num_args,
    })
}

impl ScalarLongCallback {
    /// Recognize an exact two-argument total-order comparator. Direct Compare
    /// already returns ordering; subtraction has the same sign for raw Long
    /// inputs even when canonical PHP widens overflow to Double.
    #[inline(always)]
    pub(crate) unsafe fn exact_sort_order(&self) -> Option<ScalarLongSortOrder> {
        let plan = &*self.plan;
        if plan.select.is_some()
            || plan.program.operations.len() != 1
            || plan.program.output_count != 1
            || plan.program.outputs[0] != ScalarLongSource::Temporary(0)
        {
            return None;
        }
        let operation = plan.program.operations[0];
        let order = match (operation.lhs, operation.rhs) {
            (ScalarLongSource::Input(0), ScalarLongSource::Input(1)) => {
                ScalarLongSortOrder::Ascending
            }
            (ScalarLongSource::Input(1), ScalarLongSource::Input(0)) => {
                ScalarLongSortOrder::Descending
            }
            _ => return None,
        };
        match operation.kind {
            ScalarLongOpKind::Subtract | ScalarLongOpKind::Compare => Some(order),
            _ => None,
        }
    }

    /// Evaluate already-unboxed Long arguments without recording a completed
    /// PHP call. Transactional pipeline consumers record their totals only
    /// after the complete fused span succeeds.
    #[inline(always)]
    pub(crate) unsafe fn evaluate_longs(&self, arguments: &[i64]) -> Option<i64> {
        if arguments.len() != self.public_num_args {
            return None;
        }
        let mut scalar_arguments = [0i64; 8];
        scalar_arguments[..arguments.len()].copy_from_slice(arguments);
        evaluate_scalar_long_plan(&*self.plan, &scalar_arguments)
    }

    #[inline(always)]
    pub(crate) unsafe fn record_calls(&self, count: u64) {
        record_scalar_calls_bulk(&*self.common, count);
    }
}

/// Call a PHP function from borrowed arguments without first materializing an
/// intermediate `Vec<Value>`. Each value is cloned exactly once, directly into
/// its destination CV slot in the new call frame.
pub fn call_function_iter<'a, I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
) -> Result<Value, VmError>
where
    I: Iterator<Item = &'a Value>,
{
    let (return_value, _) =
        call_function_value_iter::<_, false>(eg, func_ptr, num_args, args.cloned())?;
    Ok(return_value)
}

/// Call a PHP function from owned arguments, moving every value directly into
/// the new frame. This is used after named-argument normalization and by
/// callback consumers that already own their temporary arguments.
pub fn call_function_owned_iter<I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
) -> Result<Value, VmError>
where
    I: Iterator<Item = Value>,
{
    let (return_value, _) =
        call_function_value_iter::<_, false>(eg, func_ptr, num_args, args)?;
    Ok(return_value)
}

/// Owned-argument form that reads back the first public argument after user
/// code finishes. The argument is moved into the frame as its sole owner, so
/// ordinary PHP COW mutation stays in place; the readback clone becomes the
/// reusable owner after frame cleanup.
pub fn call_function_owned_iter_readback_arg0<I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
) -> Result<(Value, Value), VmError>
where
    I: Iterator<Item = Value>,
{
    let (return_value, arg0) =
        call_function_value_iter::<_, true>(eg, func_ptr, num_args, args)?;
    Ok((return_value, arg0.unwrap_or_else(Value::null)))
}

/// Shared callback invocation path. `READBACK_ARG0` keeps the ordinary path
/// free of the extra first-public-argument clone required by `array_walk`.
fn call_function_value_iter<I, const READBACK_ARG0: bool>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    mut args: I,
) -> Result<(Value, Option<Value>), VmError>
where
    I: Iterator<Item = Value>,
{
    let saved_execute_data = eg.current_execute_data.get();
    let frame = eg.vm_stack.push_call_frame(
        func_ptr,
        num_args as u32,
        num_args as u32,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    let mut return_value = Value::null();

    unsafe {
        (*frame).return_value = &mut return_value;
        // prev=null so Return exits execute_ex instead of continuing in caller
    }

    // Write args into CV slots — fresh uninitialized slots, use init (no drop).
    for i in 0..num_args {
        let arg = args
            .next()
            .expect("callback argument iterator shorter than declared length");
        unsafe { callback_arg_init(frame, i, arg) };
    }
    debug_assert!(
        args.next().is_none(),
        "callback argument iterator longer than declared length"
    );

    let execution_result = match unsafe { (*func_ptr).fn_type } {
        FunctionType::User => {
            let user = unsafe { &*(func_ptr as *const UserFunction) };
            unsafe { (*frame).opline = user.op_array.instructions.as_ptr() };
            eg.current_execute_data.set(frame);
            execute_ex(eg, frame)
        }
        FunctionType::Internal => {
            let internal = unsafe {
                &*(func_ptr as *const super::function::InternalFunction)
            };
            unsafe { std::ptr::drop_in_place(&mut return_value as *mut Value) };
            (internal.handler)(frame, &mut return_value, eg)
        }
        FunctionType::Undef => {
            eg.exception = Some(make_error_value("Error", "Call to undefined function"));
            Ok(())
        }
    };

    let arg0 = if READBACK_ARG0 {
        let arg0_cv = unsafe { (*func_ptr).sig.param_cv_index(0) } as usize;
        Some(if num_args > arg0_cv {
            unsafe { (*frame).cv(arg0_cv as u32).clone() }
        } else {
            Value::null()
        })
    } else {
        None
    };
    let callback_threw = eg.exception.is_some();

    // Always restore and pop the callback frame, including fatal/error paths.
    eg.current_execute_data.set(saved_execute_data);
    unsafe { cleanup_frame_slots(frame) };
    eg.vm_stack.pop_call_frame(frame);

    execution_result?;

    // A PHP exception stays in ExecutorGlobals for the calling opcode to
    // handle. Callback consumers stop iterating and ignore the partial return.
    if callback_threw {
        Ok((Value::null(), arg0))
    } else {
        Ok((return_value, arg0))
    }
}

/// Like `call_function`, but reads back the first public argument before frame
/// cleanup (CV(0) for functions, CV(1) after a method's hidden `$this`).
/// Used by `array_walk` to capture mutations made by `function (&$val, $key)` callbacks.
/// Returns `(return_value, modified_arg0)`.
pub fn call_function_readback_arg0(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    args: &[Value],
) -> Result<(Value, Value), VmError> {
    call_function_readback_arg0_iter(eg, func_ptr, args.len(), args.iter())
}

/// Borrowed-argument form of `call_function_readback_arg0`.
pub fn call_function_readback_arg0_iter<'a, I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
) -> Result<(Value, Value), VmError>
where
    I: Iterator<Item = &'a Value>,
{
    let (return_value, arg0) =
        call_function_value_iter::<_, true>(eg, func_ptr, num_args, args.cloned())?;
    Ok((return_value, arg0.unwrap_or_else(Value::null)))
}

/// Resume a generator: set up frame, copy state, execute until yield/return.
/// The generator's state is updated in place.
pub fn resume_generator(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
) -> Result<(), VmError> {
    use crate::vm::generator::GeneratorState;

    {
        let gen_data = gen_ref.borrow();
        match gen_data.state {
            GeneratorState::Completed => return Ok(()),
            GeneratorState::Running => {
                return Err(VmError::Fatal("Cannot resume an already running generator".into()));
            }
            _ => {}
        }
    }

    // Handle yield from delegation
    {
        use crate::vm::generator::YieldFromDelegate;
        let has_delegate = gen_ref.borrow().delegate.is_some();
        if has_delegate {
            let delegate = gen_ref.borrow_mut().delegate.take();
            match delegate {
                Some(YieldFromDelegate::Generator(inner_gen_ref)) => {
                    // Forward send value to inner generator
                    resume_generator(eg, &inner_gen_ref, send_value)?;

                    let inner_state = inner_gen_ref.borrow().state;
                    if inner_state == GeneratorState::Completed {
                        // Inner generator done — remove delegate, resume outer with return value
                        let ret_val = inner_gen_ref.borrow().return_value.clone();
                        gen_ref.borrow_mut().delegate = None;

                        // Resume the outer generator at the YieldFrom instruction
                        // It will advance past it. We need to write the return value
                        // to the result slot. We'll do this by resuming normally
                        // but first advancing ip past the YieldFrom and writing result.
                        {
                            let mut gen_data = gen_ref.borrow_mut();
                            // ip_offset points to YieldFrom instruction, advance past it
                            gen_data.ip_offset += 1;
                            gen_data.state = GeneratorState::Suspended;
                            // Store return value in send_value to be written to result slot
                            // We'll handle this by writing it after frame setup below
                        }

                        // Now do a normal resume, but we need to write ret_val to the
                        // YieldFrom result TMP. We handle this by writing it after frame setup.
                        // Actually, let's just set ip_offset-1 to point to YieldFrom so the
                        // send value write logic handles it... but it checks for OpCode::Yield.
                        // Better approach: resume the generator normally and write ret_val
                        // to the YieldFrom's result TMP slot manually.
                        let func_ptr = gen_ref.borrow().func;
                        let user = unsafe { &*(func_ptr as *const UserFunction) };

                        gen_ref.borrow_mut().state = GeneratorState::Running;
                        let saved_execute_data = eg.current_execute_data.get();
                        let frame = eg.vm_stack.push_call_frame(
                            func_ptr,
                            0,
                            0,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                        let mut dummy_return = Value::null();
                        unsafe {
                            (*frame).return_value = &mut dummy_return;
                        }

                        {
                            let gen_data = gen_ref.borrow();
                            for (i, val) in gen_data.cv_values.iter().enumerate() {
                                let slot = unsafe { (*frame).cv_mut(i as u32) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
                            }
                            for (i, val) in gen_data.tmp_values.iter().enumerate() {
                                let slot = unsafe { (*frame).tmp_mut(i as u32) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
                            }
                            unsafe {
                                (*frame).opline = user.op_array.instructions.as_ptr().add(gen_data.ip_offset);
                            }
                        }

                        // Write return value to the YieldFrom result slot
                        {
                            let result_slot = gen_ref.borrow().yield_from_result_slot;
                            let yield_from_instr = &user.op_array.instructions[gen_ref.borrow().ip_offset - 1];
                            if yield_from_instr.result_type != OpType::Unused {
                                let slot = unsafe { (*frame).slot_mut(result_slot) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, ret_val) };
                            }
                        }

                        let saved_active = eg.active_generator.take();
                        eg.active_generator = Some(gen_ref.clone());
                        eg.current_execute_data.set(frame);
                        let result = execute_ex(eg, frame);
                        eg.current_execute_data.set(saved_execute_data);
                        eg.active_generator = saved_active;
                        if result.is_err() {
                            gen_ref.borrow_mut().state = GeneratorState::Completed;
                        }
                        return result;
                    } else {
                        // Inner generator yielded again — copy its value/key to outer
                        let mut gen_data = gen_ref.borrow_mut();
                        let inner = inner_gen_ref.borrow();
                        gen_data.value = inner.value.clone();
                        gen_data.key = inner.key.clone();
                        drop(inner);
                        gen_data.delegate = Some(YieldFromDelegate::Generator(inner_gen_ref));
                        gen_data.state = GeneratorState::Suspended;
                        return Ok(());
                    }
                }
                Some(YieldFromDelegate::Array(entries, pos)) => {
                    if pos >= entries.len() {
                        // Array exhausted — remove delegate, resume outer
                        gen_ref.borrow_mut().delegate = None;
                        {
                            let mut gen_data = gen_ref.borrow_mut();
                            gen_data.ip_offset += 1;
                            gen_data.state = GeneratorState::Suspended;
                        }

                        let func_ptr = gen_ref.borrow().func;
                        let user = unsafe { &*(func_ptr as *const UserFunction) };

                        gen_ref.borrow_mut().state = GeneratorState::Running;
                        let saved_execute_data = eg.current_execute_data.get();
                        let frame = eg.vm_stack.push_call_frame(
                            func_ptr,
                            0,
                            0,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                        let mut dummy_return = Value::null();
                        unsafe {
                            (*frame).return_value = &mut dummy_return;
                        }

                        {
                            let gen_data = gen_ref.borrow();
                            for (i, val) in gen_data.cv_values.iter().enumerate() {
                                let slot = unsafe { (*frame).cv_mut(i as u32) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
                            }
                            for (i, val) in gen_data.tmp_values.iter().enumerate() {
                                let slot = unsafe { (*frame).tmp_mut(i as u32) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
                            }
                            unsafe {
                                (*frame).opline = user.op_array.instructions.as_ptr().add(gen_data.ip_offset);
                            }
                        }

                        // Write null to YieldFrom result (arrays return null)
                        {
                            let result_slot = gen_ref.borrow().yield_from_result_slot;
                            let yield_from_instr = &user.op_array.instructions[gen_ref.borrow().ip_offset - 1];
                            if yield_from_instr.result_type != OpType::Unused {
                                let slot = unsafe { (*frame).slot_mut(result_slot) };
                                unsafe { frame_restore_slot(frame, slot as *mut Value, Value::null()) };
                            }
                        }

                        let saved_active = eg.active_generator.take();
                        eg.active_generator = Some(gen_ref.clone());
                        eg.current_execute_data.set(frame);
                        let result = execute_ex(eg, frame);
                        eg.current_execute_data.set(saved_execute_data);
                        eg.active_generator = saved_active;
                        if result.is_err() {
                            gen_ref.borrow_mut().state = GeneratorState::Completed;
                        }
                        return result;
                    } else {
                        // Yield next array element
                        let mut gen_data = gen_ref.borrow_mut();
                        let (ref key, ref val) = entries[pos];
                        gen_data.value = val.clone();
                        gen_data.key = match key {
                            crate::value::ArrayKey::Int(i) => Value::long(*i),
                            crate::value::ArrayKey::String(s) => Value::string(s.clone()),
                        };
                        gen_data.delegate = Some(YieldFromDelegate::Array(entries, pos + 1));
                        gen_data.state = GeneratorState::Suspended;
                        return Ok(());
                    }
                }
                None => unreachable!(),
            }
        }
    }

    // Mark as running
    gen_ref.borrow_mut().state = GeneratorState::Running;

    let func_ptr = gen_ref.borrow().func;
    let user = unsafe { &*(func_ptr as *const UserFunction) };
    let saved_execute_data = eg.current_execute_data.get();

    // Push a frame for the generator
    let frame = eg.vm_stack.push_call_frame(
        func_ptr,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    let mut dummy_return = Value::null();
    unsafe {
        (*frame).return_value = &mut dummy_return;
    }

    // Copy saved CV values into frame
    {
        let gen_data = gen_ref.borrow();
        for (i, val) in gen_data.cv_values.iter().enumerate() {
            let slot = unsafe { (*frame).cv_mut(i as u32) };
            unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
        }
        for (i, val) in gen_data.tmp_values.iter().enumerate() {
            let slot = unsafe { (*frame).tmp_mut(i as u32) };
            unsafe { frame_restore_slot(frame, slot as *mut Value, val.clone()) };
        }

        // Set instruction pointer
        unsafe {
            (*frame).opline = user.op_array.instructions.as_ptr().add(gen_data.ip_offset);
        }
    }

    // If resuming from a yield (not first call), write send value to the
    // previous yield's result TMP. The yield instruction at ip_offset-1
    // told us its result slot.
    {
        let gen_data = gen_ref.borrow();
        if gen_data.state == GeneratorState::Running && gen_data.ip_offset > 0 {
            // The yield instruction is at ip_offset - 1
            let yield_instr = &user.op_array.instructions[gen_data.ip_offset - 1];
            if yield_instr.opcode == crate::vm::opcode::OpCode::Yield
                && yield_instr.result_type != OpType::Unused
            {
                let tmp_slot = unsafe { (*frame).slot_mut(yield_instr.result as u32) };
                unsafe { frame_restore_slot(frame, tmp_slot as *mut Value, send_value.clone()) };
            }
        }
    }

    // Set active generator so Yield/Return can find it
    let saved_active = eg.active_generator.take();
    eg.active_generator = Some(gen_ref.clone());

    eg.current_execute_data.set(frame);
    let result = execute_ex(eg, frame);

    // Restore state
    eg.current_execute_data.set(saved_execute_data);
    eg.active_generator = saved_active;

    // Clean up frame (CV/TMP already saved by Yield handler)
    // Note: Yield handler already cleaned up the frame, but if Return happened
    // or an error occurred, the frame might still be allocated.
    // The Yield/Return handlers pop the frame themselves, so we only need
    // to handle the error case.
    if result.is_err() {
        gen_ref.borrow_mut().state = GeneratorState::Completed;
    }

    result
}

/// Enter the canonical executor without imposing its top-level cleanup policy.
///
/// The opt-in coroutine runtime owns the detached stack and must distinguish a
/// cooperative suspension from completion before it can clean or recycle the
/// frame chain. Keeping this wrapper feature-gated leaves the ordinary entry
/// point and dispatch loop unchanged in non-coroutine builds.
#[cfg(feature = "coroutines")]
pub(crate) fn execute_coroutine_frame(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    boundary: *mut ExecuteData,
) -> Result<(), VmError> {
    let mut entry = frame;
    loop {
        execute_ex(eg, entry)?;
        if entry == boundary {
            return Ok(());
        }
        entry = eg.current_execute_data.get();
        if entry.is_null() {
            return Ok(());
        }
    }
}
