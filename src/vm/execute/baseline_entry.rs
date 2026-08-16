// Kept in the execute module through include! so this structural split does not change visibility or code generation.

pub fn execute(eg: &mut ExecutorGlobals, main_func: &UserFunction) -> Result<Value, VmError> {
    crate::value::begin_object_handle_request();
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

    let execution = execute_ex(eg, frame);
    crate::value::end_object_handle_request();
    execution?;

    #[cfg(debug_assertions)]
    super::hot::dump_bail_stats();

    eg.current_execute_data.set(unsafe { (*frame).prev_execute_data });
    run_frame_destructors(eg, frame)?;
    unsafe { cleanup_frame_slots(frame) };
    pop_vm_call_frame(eg, frame);

    crate::stdlib::flush_all_output_buffers(eg)?;

    // Check for uncaught exception that propagated through execute_ex
    if let Some(exc) = eg.exception.take() {
        let (class_name, message, located_trace) = if let Some(obj) = exc.as_object() {
            let cls = obj.class_name.clone();
            let msg = obj.get_property("message")
                .map(|v| v.echo_to_string())
                .unwrap_or_default();
            let located_trace = obj
                .get_property("file")
                .and_then(Value::as_str)
                .filter(|file| !file.is_empty())
                .zip(obj.get_property("line").and_then(Value::as_long))
                .filter(|(_, line)| *line > 0)
                .zip(
                    obj.get_property("trace")
                        .and_then(Value::as_array)
                        .cloned(),
                )
                .map(|((file, line), trace)| (file.to_string(), line, trace));
            (cls, msg, located_trace)
        } else {
            (
                std::rc::Rc::from("Exception"),
                exc.echo_to_string(),
                None,
            )
        };
        let message_suffix = if message.is_empty() {
            String::new()
        } else {
            format!(": {message}")
        };
        if let Some((file, line, trace)) = located_trace {
            let trace = crate::vm::trace::format_throwable_trace(&trace);
            return Err(VmError::Fatal(format!(
                "Uncaught {class_name}{message_suffix} in {file}:{line}\nStack trace:\n{trace}\n  thrown in {file} on line {line}"
            )));
        }
        return Err(VmError::Fatal(format!(
            "Uncaught {class_name}{message_suffix}"
        )));
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
    let (return_value, _) = call_function_value_iter::<_, false>(
        eg,
        func_ptr,
        num_args,
        args.cloned(),
        0,
        None,
        0,
        None,
    )?;
    Ok(return_value)
}

/// Closure-aware detached callback entry. Captures remain ordinary trailing
/// arguments, while bound `$this` and lexical scope are frame metadata rather
/// than public parameters.
pub(crate) fn call_function_iter_with_context<'a, I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
    called_scope_class_id: u32,
    bound_this: Option<&Value>,
    capture_count: usize,
) -> Result<Value, VmError>
where
    I: Iterator<Item = &'a Value>,
{
    let (return_value, _) = call_function_value_iter::<_, false>(
        eg,
        func_ptr,
        num_args,
        args.cloned(),
        called_scope_class_id,
        bound_this.cloned(),
        capture_count,
        None,
    )?;
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
        call_function_value_iter::<_, false>(eg, func_ptr, num_args, args, 0, None, 0, None)?;
    Ok(return_value)
}

/// Owned-argument closure entry retaining bound object and lexical scope.
pub(crate) fn call_function_owned_iter_with_context<I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
    called_scope_class_id: u32,
    bound_this: Option<Value>,
    capture_count: usize,
) -> Result<Value, VmError>
where
    I: Iterator<Item = Value>,
{
    let (return_value, _) = call_function_value_iter::<_, false>(
        eg,
        func_ptr,
        num_args,
        args,
        called_scope_class_id,
        bound_this,
        capture_count,
        None,
    )?;
    Ok(return_value)
}

pub(crate) fn call_function_owned_iter_with_context_and_named<I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
    called_scope_class_id: u32,
    bound_this: Option<Value>,
    capture_count: usize,
    named_variadic: Vec<(String, Value)>,
) -> Result<Value, VmError>
where
    I: Iterator<Item = Value>,
{
    let (return_value, _) = call_function_value_iter::<_, false>(
        eg,
        func_ptr,
        num_args,
        args,
        called_scope_class_id,
        bound_this,
        capture_count,
        Some(named_variadic),
    )?;
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
        call_function_value_iter::<_, true>(eg, func_ptr, num_args, args, 0, None, 0, None)?;
    Ok((return_value, arg0.unwrap_or_else(Value::null)))
}

/// Owned closure entry retaining context and reading back its first argument.
pub(crate) fn call_function_owned_iter_readback_arg0_with_context<I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
    called_scope_class_id: u32,
    bound_this: Option<Value>,
) -> Result<(Value, Value), VmError>
where
    I: Iterator<Item = Value>,
{
    let (return_value, arg0) = call_function_value_iter::<_, true>(
        eg,
        func_ptr,
        num_args,
        args,
        called_scope_class_id,
        bound_this,
        0,
        None,
    )?;
    Ok((return_value, arg0.unwrap_or_else(Value::null)))
}

/// Shared callback invocation path. `READBACK_ARG0` keeps the ordinary path
/// free of the extra first-public-argument clone required by `array_walk`.
fn call_function_value_iter<I, const READBACK_ARG0: bool>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    mut args: I,
    called_scope_class_id: u32,
    bound_this: Option<Value>,
    capture_count: usize,
    named_variadic: Option<Vec<(String, Value)>>,
) -> Result<(Value, Option<Value>), VmError>
where
    I: Iterator<Item = Value>,
{
    let saved_execute_data = eg.current_execute_data.get();
    // Detached callback entries (Iterator, ArrayAccess, array_* callbacks, ...)
    // bypass DoFcall. Publish the suspended caller's current global-scope CVs
    // before a user callback that may execute `global $name`; otherwise the
    // callback can bind to an older ExecutorGlobals snapshot.
    let user_callee = unsafe { ((*func_ptr).fn_type == FunctionType::User).then(|| &*(func_ptr as *const UserFunction)) };
    if let Some(user) = user_callee
        && user.op_array.may_access_globals
        && !saved_execute_data.is_null()
    {
        unsafe {
            let caller = &mut *saved_execute_data;
            sync_dirty_globals_to_frame(eg, caller);
            let caller_op_array = caller.op_array();
            let vars = if !caller_op_array.main_scope_vars.is_empty() {
                &caller_op_array.main_scope_vars
            } else {
                &caller_op_array.global_vars
            };
            for (cv, name) in vars {
                globals_set(&mut eg.globals, name, caller.cv(*cv).clone());
            }
        }
    }
    // SAFETY: the detached-call boundary receives a resolved registered
    // function descriptor that remains live for the synchronous invocation.
    let (signature, function_type) = unsafe { (&(*func_ptr).sig, (*func_ptr).fn_type) };
    let this_offset = signature.this_offset as usize;
    let positional_public_num_args = num_args.saturating_sub(this_offset + capture_count);
    let public_num_args = positional_public_num_args
        .saturating_add(named_variadic.as_ref().map_or(0, Vec::len));
    if public_num_args < signature.required_num_args as usize {
        let common = unsafe { &*func_ptr };
        let required = signature.required_num_args;
        let relation = if common.fn_type == FunctionType::Internal {
            if signature.is_variadic || signature.public_arity() > required {
                "at least"
            } else {
                "exactly"
            }
        } else if signature.public_arity() > required {
            "at least"
        } else {
            "exactly"
        };
        let name = displayed_function_name(eg, func_ptr);
        let message = if common.fn_type == FunctionType::Internal {
            let noun = if required == 1 {
                "argument"
            } else {
                "arguments"
            };
            format!(
                "{name}() expects {relation} {required} {noun}, {public_num_args} given"
            )
        } else {
            format!(
                "Too few arguments to function {name}(), {public_num_args} passed and {relation} {required} expected"
            )
        };
        eg.exception = Some(make_error_value("ArgumentCountError", &message));
        return Ok((Value::null(), None));
    }
    if function_type == FunctionType::Internal
        && !signature.is_variadic
        && public_num_args > signature.public_arity() as usize
    {
        eg.exception = Some(too_many_internal_arguments_error(
            eg,
            func_ptr,
            signature,
            public_num_args as u32,
        ));
        return Ok((Value::null(), None));
    }
    let capture_destination = signature.parameter_cv_count() as usize;
    let storage_num_args = if capture_count == 0 {
        num_args
    } else {
        num_args.max(capture_destination + capture_count)
    };
    let frame = eg.vm_stack.push_call_frame(
        func_ptr,
        storage_num_args as u32,
        public_num_args as u32,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    let mut return_value = Value::null();

    // SAFETY: `frame` is a fresh compiler-sized activation. All argument and
    // variadic destinations are uninitialized slots described by `func_ptr`.
    unsafe {
        (*frame).return_value = &mut return_value;
        // prev=null so Return exits execute_ex instead of continuing in caller

        // Write args into CV slots — fresh uninitialized slots, use init (no drop).
        for i in 0..num_args {
            let arg = args
                .next()
                .expect("callback argument iterator shorter than declared length");
            callback_arg_init(frame, i, arg);
        }
        debug_assert!(
            args.next().is_none(),
            "callback argument iterator longer than declared length"
        );

        if called_scope_class_id != 0 {
            publish_late_static_call_class_id(eg, frame, called_scope_class_id);
        }
        initialize_bound_this_frame(frame, func_ptr, bound_this);

        // Detached callback entry bypasses DoFcall, whose full path normally
        // materializes the variadic bucket. Internal handlers use the same ABI in
        // both entry modes, so pack their trailing public arguments here before
        // dispatching the handler.
        let saved_captures = (capture_count != 0).then(|| {
            let start = this_offset + positional_public_num_args;
            (0..capture_count)
                .map(|index| (*frame).cv((start + index) as u32).clone_closure_capture())
                .collect::<Vec<_>>()
        });

        if (*func_ptr).sig.is_variadic {
            let sig = &(*func_ptr).sig;
            let fixed = sig.public_arity() as usize;
            let extra_count = positional_public_num_args.saturating_sub(fixed);
            let mut variadic = PhpArray::with_packed_capacity(extra_count);
            let by_reference = sig.is_param_by_ref(fixed as u32);
            for index in 0..extra_count {
                let argument = (*frame).cv((this_offset + fixed + index) as u32);
                let value = if by_reference && argument.is_owned_reference() {
                    argument.clone_owned_reference_alias()
                } else if by_reference && argument.is_reference() {
                    Value::reference(argument.as_ref_ptr())
                } else {
                    argument.clone()
                };
                variadic.push(value);
            }
            if let Some(named) = named_variadic {
                for (name, value) in named {
                    variadic.set_str(&name, value);
                }
            }
            let destination = (*frame).cv_mut(sig.variadic_cv_index) as *mut Value;
            frame_slot_set(frame, destination, Value::array(variadic));
        }

        if let Some(captures) = saved_captures {
            for (index, capture) in captures.into_iter().enumerate() {
                let destination = (*frame).cv_mut((capture_destination + index) as u32);
                frame_slot_set(frame, destination as *mut Value, capture);
            }
        }

        // Generator functions invoked through this detached callback entry do
        // not execute their body yet; publish the same suspended object that
        // DoFcall creates while the fresh frame and function pointer are valid.
        let user = ((*func_ptr).fn_type == FunctionType::User)
            .then(|| &*(func_ptr as *const UserFunction));
        if let Some(user) = user
            && user.op_array.is_generator
        {
            use crate::vm::generator::{Generator, new_generator_ref};

            let mut arguments = Vec::with_capacity(num_args);
            for index in 0..num_args {
                arguments.push((*frame).cv(index as u32).clone());
            }
            let mut generator = Generator::new(
                func_ptr,
                arguments,
                user.op_array.num_cvs,
                user.op_array.num_temps,
            );
            generator.called_scope_class_id = called_scope_class_id;
            let generator_ref = new_generator_ref(generator);
            let mut object = PhpObject::dynamic("Generator".to_string(), 0, HashMap::new());
            object.generator = Some(generator_ref);
            let generator_value = Value::object(object);
            let return_hint = &(*func_ptr).sig.return_type_hint;
            let callee_class = eg.declaring_class_of(func_ptr);
            if !check_type_hint(
                &generator_value,
                return_hint,
                eg,
                user.op_array.strict_types,
                callee_class,
            ) {
                eg.exception = Some(make_error_value(
                    "TypeError",
                    &format!(
                        "Generator return type must be a supertype of Generator, {} given",
                        return_hint.display_name()
                    ),
                ));
            }
            let arg0 = if READBACK_ARG0 && num_args > 0 {
                Some((*frame).cv(0).clone())
            } else {
                None
            };
            eg.current_execute_data.set(saved_execute_data);
            cleanup_frame_slots(frame);
            pop_vm_call_frame(eg, frame);
            return if eg.exception.is_some() {
                Ok((Value::null(), arg0))
            } else {
                Ok((generator_value, arg0))
            };
        }
    }

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
    pop_vm_call_frame(eg, frame);

    // Complete the other half of the ordinary call boundary: writes through
    // `global` in a detached callback must become visible in the suspended
    // caller before its next opcode executes.
    if !saved_execute_data.is_null() {
        unsafe { sync_dirty_globals_to_frame(eg, &mut *saved_execute_data) };
    }

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
    let (return_value, arg0) = call_function_value_iter::<_, true>(
        eg,
        func_ptr,
        num_args,
        args.cloned(),
        0,
        None,
        0,
        None,
    )?;
    Ok((return_value, arg0.unwrap_or_else(Value::null)))
}

/// Observable result of one generator resume boundary.
pub(crate) enum GeneratorResumeOutcome {
    /// The generator either yielded or completed normally. Its state carries
    /// the exact distinction without cloning another payload.
    Advanced,
    /// A PHP exception escaped the detached generator frame. The caller owns
    /// reinjection into its live frame (foreach, yield-from or an internal
    /// Generator method call).
    Threw(Value),
}

/// Resume a generator: set up frame, copy state, execute until yield/return.
/// The generator's state is updated in place and detached exceptions are
/// returned explicitly rather than left in the executor sidecar.
pub(crate) fn resume_generator(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
) -> Result<GeneratorResumeOutcome, VmError> {
    use crate::vm::generator::GeneratorState;

    {
        let gen_data = gen_ref.borrow();
        match gen_data.state {
            GeneratorState::Completed => return Ok(GeneratorResumeOutcome::Advanced),
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
                    match resume_generator(eg, &inner_gen_ref, send_value)? {
                        GeneratorResumeOutcome::Advanced => {}
                        GeneratorResumeOutcome::Threw(exception) => {
                            gen_ref.borrow_mut().delegate = None;
                            let (frame, saved_execute_data) =
                                materialize_generator_frame(eg, gen_ref);
                            return execute_resumed_generator_frame(
                                eg,
                                gen_ref,
                                frame,
                                saved_execute_data,
                                Some(exception),
                            );
                        }
                    }

                    let inner_state = inner_gen_ref.borrow().state;
                    if inner_state == GeneratorState::Completed {
                        // Advance the outer frame past YieldFrom and publish
                        // the delegate's getReturn() value into its result TMP.
                        let ret_val = inner_gen_ref.borrow().return_value.clone();
                        gen_ref.borrow_mut().delegate = None;
                        {
                            let mut gen_data = gen_ref.borrow_mut();
                            gen_data.ip_offset += 1;
                        }
                        let (frame, saved_execute_data) =
                            materialize_generator_frame(eg, gen_ref);
                        restore_yield_from_result(frame, gen_ref, ret_val);

                        return execute_resumed_generator_frame(
                            eg,
                            gen_ref,
                            frame,
                            saved_execute_data,
                            None,
                        );
                    } else {
                        // Inner generator yielded again — copy its value/key to outer
                        let mut gen_data = gen_ref.borrow_mut();
                        let inner = inner_gen_ref.borrow();
                        gen_data.value = inner.value.clone();
                        gen_data.key = inner.key.clone();
                        drop(inner);
                        gen_data.delegate = Some(YieldFromDelegate::Generator(inner_gen_ref));
                        gen_data.state = GeneratorState::Suspended;
                        return Ok(GeneratorResumeOutcome::Advanced);
                    }
                }
                Some(YieldFromDelegate::Array(entries, pos)) => {
                    if pos >= entries.len() {
                        // Array exhausted — remove delegate, resume outer
                        gen_ref.borrow_mut().delegate = None;
                        {
                            let mut gen_data = gen_ref.borrow_mut();
                            gen_data.ip_offset += 1;
                        }

                        let (frame, saved_execute_data) =
                            materialize_generator_frame(eg, gen_ref);
                        restore_yield_from_result(frame, gen_ref, Value::null());

                        return execute_resumed_generator_frame(
                            eg,
                            gen_ref,
                            frame,
                            saved_execute_data,
                            None,
                        );
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
                        return Ok(GeneratorResumeOutcome::Advanced);
                    }
                }
                None => unreachable!(),
            }
        }
    }

    let (frame, saved_execute_data) = materialize_generator_frame(eg, gen_ref);
    restore_yield_send_value(frame, gen_ref, send_value);
    execute_resumed_generator_frame(eg, gen_ref, frame, saved_execute_data, None)
}

/// Materialize one detached frame from the generator snapshot. All resume
/// paths use this function so slot restoration and frame ownership cannot
/// drift between normal yield, delegated return and delegated exception.
fn materialize_generator_frame(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
) -> (*mut ExecuteData, *mut ExecuteData) {
    gen_ref.borrow_mut().state = crate::vm::generator::GeneratorState::Running;
    let func_ptr = gen_ref.borrow().func;
    let user = unsafe { &*(func_ptr as *const UserFunction) };
    let saved_execute_data = eg.current_execute_data.get();
    let frame = eg.vm_stack.push_call_frame(
        func_ptr,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    unsafe { (*frame).return_value = std::ptr::null_mut() };

    let gen_data = gen_ref.borrow();
    publish_late_static_call_class_id(eg, frame, gen_data.called_scope_class_id);
    for (i, value) in gen_data.cv_values.iter().enumerate() {
        let slot = unsafe { (*frame).cv_mut(i as u32) };
        unsafe { frame_restore_slot(frame, slot as *mut Value, value.clone()) };
    }
    for (i, value) in gen_data.tmp_values.iter().enumerate() {
        let slot = unsafe { (*frame).tmp_mut(i as u32) };
        unsafe { frame_restore_slot(frame, slot as *mut Value, value.clone()) };
    }
    unsafe {
        (*frame).opline = user
            .op_array
            .instructions
            .as_ptr()
            .add(gen_data.ip_offset)
    };
    drop(gen_data);
    (frame, saved_execute_data)
}

fn restore_yield_send_value(
    frame: *mut ExecuteData,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
) {
    let gen_data = gen_ref.borrow();
    if gen_data.ip_offset == 0 {
        return;
    }
    let user = unsafe { &*(gen_data.func as *const UserFunction) };
    let yield_instruction = &user.op_array.instructions[gen_data.ip_offset - 1];
    if yield_instruction.opcode == crate::vm::opcode::OpCode::Yield
        && yield_instruction.result_type != OpType::Unused
    {
        let slot = unsafe { (*frame).slot_mut(yield_instruction.result as u32) };
        unsafe { frame_restore_slot(frame, slot as *mut Value, send_value) };
    }
}

fn restore_yield_from_result(
    frame: *mut ExecuteData,
    gen_ref: &crate::vm::generator::GeneratorRef,
    value: Value,
) {
    let gen_data = gen_ref.borrow();
    let user = unsafe { &*(gen_data.func as *const UserFunction) };
    let yield_from_instruction = &user.op_array.instructions[gen_data.ip_offset - 1];
    if yield_from_instruction.result_type != OpType::Unused {
        let slot = unsafe { (*frame).slot_mut(gen_data.yield_from_result_slot) };
        unsafe { frame_restore_slot(frame, slot as *mut Value, value) };
    }
}

/// Execute one materialized generator frame and restore every executor
/// sidecar, including feature-gated generic contracts, on yield or return.
fn execute_resumed_generator_frame(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    frame: *mut ExecuteData,
    saved_execute_data: *mut ExecuteData,
    injected_exception: Option<Value>,
) -> Result<GeneratorResumeOutcome, VmError> {
    let saved_active = eg.active_generator.take();
    // A caller executing `finally` may already carry an exception that must
    // stay invisible to the detached generator. Normal advancement restores
    // it; a new escaped exception or VM failure supersedes it.
    let saved_exception = eg.exception.take();
    eg.active_generator = Some(gen_ref.clone());
    activate_generator_generic_context(eg, gen_ref, frame);
    eg.current_execute_data.set(frame);

    let result = if let Some(exception) = injected_exception {
        match throw_in_frame(eg, frame, exception) {
            ThrowResult::Handled(new_frame, _) => execute_ex(eg, new_frame),
            ThrowResult::Unhandled(exception) => {
                eg.exception = Some(exception);
                Ok(())
            }
        }
    } else {
        execute_ex(eg, frame)
    };
    let escaped_exception = eg.exception.take();

    cleanup_detached_generator_frames(eg, frame);
    eg.current_execute_data.set(saved_execute_data);
    eg.active_generator = saved_active;
    if let Err(error) = result {
        close_failed_generator(gen_ref);
        return Err(error);
    }
    if let Some(exception) = escaped_exception {
        close_failed_generator(gen_ref);
        return Ok(GeneratorResumeOutcome::Threw(exception));
    }
    if gen_ref.borrow().state == crate::vm::generator::GeneratorState::Running {
        close_failed_generator(gen_ref);
        return Err(VmError::Fatal(
            "Generator resume returned without yielding or completing".into(),
        ));
    }
    eg.exception = saved_exception;
    Ok(GeneratorResumeOutcome::Advanced)
}

fn close_failed_generator(gen_ref: &crate::vm::generator::GeneratorRef) {
    let mut generator = gen_ref.borrow_mut();
    generator.state = crate::vm::generator::GeneratorState::Completed;
    generator.value = Value::null();
    generator.key = Value::null();
    generator.delegate = None;
}

/// A detached generator owns every frame above and including `root`. Normal
/// yield/return leaves the root allocated; an unhandled exception can also
/// leave nested callees above it. Reclaim the complete chain exactly once.
fn cleanup_detached_generator_frames(eg: &mut ExecutorGlobals, root: *mut ExecuteData) {
    let mut frame = eg.current_execute_data.get();
    while !frame.is_null() {
        let previous = unsafe { (*frame).prev_execute_data };
        eg.current_execute_data.set(previous);
        discard_generator_generic_context(eg, frame);
        unsafe {
            cleanup_pending_calls(eg, frame);
            cleanup_frame_slots(frame);
        }
        pop_vm_call_frame(eg, frame);
        if frame == root {
            break;
        }
        frame = previous;
    }
}

#[inline]
fn activate_generator_generic_context(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    frame: *mut ExecuteData,
) {
    #[cfg(feature = "php-generics-reified")]
    if let Some(context) = gen_ref.borrow().reified_context {
        eg.activate_generator_reified_context(frame as usize, context);
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    if let Some(contract) = gen_ref.borrow().generic_member_contract.clone() {
        eg.activate_generic_member_call(frame as usize, contract);
    }

    #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
    let _ = (eg, gen_ref, frame);
}

#[inline]
fn discard_generator_generic_context(eg: &mut ExecutorGlobals, frame: *mut ExecuteData) {
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    eg.discard_generic_member_call(frame as usize);
    #[cfg(feature = "php-generics-reified")]
    eg.discard_active_reified_binding_scope(frame as usize);

    #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
    let _ = (eg, frame);
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

/// Publish a value into the suspended caller's result slot while preserving
/// the canonical heap-slot bitmap used by frame cleanup.
#[cfg(feature = "coroutines")]
pub(crate) unsafe fn write_coroutine_result(
    frame: *mut ExecuteData,
    return_value: *mut Value,
    value: Value,
) {
    if !return_value.is_null() {
        assert!(!frame.is_null());
        unsafe { frame_slot_set(frame, return_value, value) };
    }
}
