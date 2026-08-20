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

    let mut execution = execute_ex(eg, frame);
    if execution.is_ok()
        && main_func.op_array.source_file.as_ref() != "Command line code"
        && eg.exception.is_some()
        && eg.exception_handler.is_some()
    {
        let exception = eg
            .exception
            .take()
            .expect("uncaught exception handler requires a pending exception");
        // The root frame stays live until the engine callback returns: PHP
        // runs the handler before main-scope destructors, and detached callback
        // traces still terminate at the synthetic `{main}` frame.
        eg.current_execute_data.set(frame);
        match crate::stdlib::dispatch_uncaught_exception_handler(eg, frame, &exception) {
            Ok(true) => {}
            Ok(false) => {
                if eg.exception.is_none() {
                    eg.exception = Some(exception);
                }
            }
            Err(error) => execution = Err(error),
        }
    }
    if execution.is_ok() && eg.exception.is_none() && eg.shutdown_functions.is_some()
        && let Err(error) = crate::stdlib::run_shutdown_functions(eg, frame)
    {
        execution = Err(error);
    }
    crate::value::end_object_handle_request();
    execution?;

    eg.current_execute_data.set(unsafe { (*frame).prev_execute_data });
    run_frame_destructors(eg, frame)?;
    unsafe { cleanup_frame_slots(frame) };
    pop_vm_call_frame(eg, frame);

    crate::stdlib::flush_all_output_buffers(eg)?;

    // Check for uncaught exception that propagated through execute_ex.
    if let Some(exc) = eg.exception.take() {
        if exc.as_object().is_some_and(|object| {
            object.class_name.as_ref().eq_ignore_ascii_case("ParseError")
        }) {
            return Err(VmError::Parse(format_parse_error(&exc)));
        }
        return Err(VmError::Fatal(format_uncaught_throwable(eg, &exc)));
    }

    eg.finalize_pending_named_classes()?;

    Ok(return_value)
}

#[cold]
fn format_parse_error(thrown: &Value) -> String {
    let Some(object) = thrown.as_object() else {
        return thrown.echo_to_string();
    };
    let message = object
        .get_property("message")
        .map(Value::echo_to_string)
        .unwrap_or_default();
    let location = object
        .get_property("file")
        .and_then(Value::as_str)
        .filter(|file| !file.is_empty())
        .zip(object.get_property("line").and_then(Value::as_long))
        .filter(|(_, line)| *line > 0);
    match location {
        Some((file, line)) => format!("{message} in {file} on line {line}"),
        None => message,
    }
}

#[cold]
pub(crate) fn format_uncaught_throwable(eg: &ExecutorGlobals, thrown: &Value) -> String {
    format_throwable_chain(eg, thrown, true)
}

#[cold]
pub(crate) fn format_throwable_string(eg: &ExecutorGlobals, thrown: &Value) -> String {
    format_throwable_chain(eg, thrown, false)
}

#[cold]
fn format_throwable_chain(eg: &ExecutorGlobals, thrown: &Value, uncaught: bool) -> String {
    struct Segment {
        class_name: String,
        message: String,
        location: Option<(String, i64, PhpArray)>,
    }

    fn snapshot(value: &Value) -> Option<Segment> {
        let object = value.as_object()?;
        let class_name = object.class_name.to_string();
        let message = object
            .get_property("message")
            .map(Value::echo_to_string)
            .unwrap_or_default();
        let location = object
            .get_property("file")
            .and_then(Value::as_str)
            .filter(|file| !file.is_empty())
            .zip(object.get_property("line").and_then(Value::as_long))
            .zip(
                object
                    .get_property("trace")
                    .and_then(Value::as_array)
                    .cloned(),
            )
            .map(|((file, line), trace)| (file.to_string(), line, trace));
        Some(Segment {
            class_name,
            message,
            location,
        })
    }

    fn previous(eg: &ExecutorGlobals, value: &Value) -> Option<Value> {
        let object = value.as_object()?;
        let key = eg
            .find_property_visibility(&object.class_name, "previous")
            .map_or_else(
                || "previous".to_string(),
                |(_, declaring_class)| {
                    crate::runtime::mangle_private_prop(&declaring_class, "previous")
                },
            );
        object
            .get_property(&key)
            .filter(|previous| {
                previous
                    .as_object()
                    .is_some_and(|object| eg.class_is_a(&object.class_name, "Throwable"))
            })
            .cloned()
    }

    let Some(final_segment) = snapshot(thrown) else {
        let message = thrown.echo_to_string();
        return if message.is_empty() {
            if uncaught {
                "Uncaught Exception".to_string()
            } else {
                "Exception".to_string()
            }
        } else if uncaught {
            format!("Uncaught Exception: {message}")
        } else {
            format!("Exception: {message}")
        };
    };
    let final_location = final_segment
        .location
        .as_ref()
        .map(|(file, line, _)| (file.clone(), *line));
    let mut segments = vec![final_segment];
    let mut seen = std::collections::HashSet::new();
    if let Some(identity) = thrown.object_identity() {
        seen.insert(identity);
    }
    let mut current = thrown.clone();
    while let Some(candidate) = previous(eg, &current) {
        let Some(identity) = candidate.object_identity() else {
            break;
        };
        if !seen.insert(identity) {
            break;
        }
        let Some(segment) = snapshot(&candidate) else {
            break;
        };
        segments.push(segment);
        current = candidate;
    }
    segments.reverse();

    let mut rendered = String::new();
    for (index, segment) in segments.into_iter().enumerate() {
        if index == 0 && uncaught {
            rendered.push_str("Uncaught ");
        } else if index != 0 {
            rendered.push_str("\n\nNext ");
        }
        rendered.push_str(&segment.class_name);
        if !segment.message.is_empty() {
            rendered.push_str(": ");
            rendered.push_str(&segment.message);
        }
        if let Some((file, line, trace)) = segment.location {
            if segment.class_name == "TypeError"
                && segment.message.contains(", called in ")
                && segment.message.contains(" on line ")
            {
                rendered.push_str(" and defined in ");
            } else {
                rendered.push_str(" in ");
            }
            rendered.push_str(&file);
            rendered.push(':');
            rendered.push_str(&line.to_string());
            rendered.push_str("\nStack trace:\n");
            rendered.push_str(&crate::vm::trace::format_throwable_trace(
                &trace,
                crate::stdlib::exception_string_param_max_len(eg),
                eg,
            ));
        }
    }
    if uncaught && let Some((file, line)) = final_location {
        rendered.push_str("\n  thrown in ");
        rendered.push_str(&file);
        rendered.push_str(" on line ");
        rendered.push_str(&line.to_string());
    }
    rendered
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
        None,
        std::ptr::null_mut(),
        false,
        None,
    )?;
    Ok(return_value)
}

/// Enter a user callback dispatched by the active source instruction. Magic
/// property operations use this detached boundary: their body must return to
/// the opcode helper, while live/stored traces still expose the source-level
/// property access as the callback's logical caller and origin.
fn call_function_iter_from_current_site<'a, I>(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
) -> Result<Value, VmError>
where
    I: Iterator<Item = &'a Value>,
{
    let logical_caller = eg.current_execute_data.get();
    let (return_value, _) = call_function_value_iter::<_, false>(
        eg,
        func_ptr,
        num_args,
        args.cloned(),
        0,
        None,
        0,
        None,
        None,
        logical_caller,
        true,
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
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
) -> Result<Value, VmError>
where
    I: Iterator<Item = &'a Value>,
{
    let capture_start = num_args.saturating_sub(capture_count);
    let (return_value, _) = call_function_value_iter::<_, false>(
        eg,
        func_ptr,
        num_args,
        args.enumerate().map(|(index, value)| {
            if index >= capture_start {
                value.clone_closure_capture()
            } else {
                value.clone()
            }
        }),
        called_scope_class_id,
        bound_this.cloned(),
        capture_count,
        closure_static_vars,
        None,
        std::ptr::null_mut(),
        false,
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
        call_function_value_iter::<_, false>(
            eg,
            func_ptr,
            num_args,
            args,
            0,
            None,
            0,
            None,
            None,
            std::ptr::null_mut(),
            false,
            None,
        )?;
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
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
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
        closure_static_vars,
        None,
        std::ptr::null_mut(),
        false,
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
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
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
        closure_static_vars,
        Some(named_variadic),
        std::ptr::null_mut(),
        false,
        None,
    )?;
    Ok(return_value)
}

pub(crate) fn call_function_owned_iter_with_context_and_named_from<I>(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
    called_scope_class_id: u32,
    bound_this: Option<Value>,
    capture_count: usize,
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
    named_variadic: Vec<(String, Value)>,
    trace_origin: (String, usize),
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
        closure_static_vars,
        Some(named_variadic),
        logical_caller,
        false,
        Some((trace_origin.0, trace_origin.1, None)),
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
        call_function_value_iter::<_, true>(
            eg,
            func_ptr,
            num_args,
            args,
            0,
            None,
            0,
            None,
            None,
            std::ptr::null_mut(),
            false,
            None,
        )?;
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
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
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
        closure_static_vars,
        None,
        std::ptr::null_mut(),
        false,
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
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
    named_variadic: Option<Vec<(String, Value)>>,
    logical_caller: *mut ExecuteData,
    trace_caller_at_current_site: bool,
    trace_origin: Option<(String, usize, Option<&Value>)>,
) -> Result<(Value, Option<Value>), VmError>
where
    I: Iterator<Item = Value>,
{
    let saved_execute_data = eg.current_execute_data.get();
    // SAFETY: the detached-call boundary receives a resolved registered
    // function descriptor that remains live for the synchronous invocation.
    let (user_callee, signature, function_type) = unsafe {
        (
            ((*func_ptr).fn_type == FunctionType::User)
                .then(|| &*(func_ptr as *const UserFunction)),
            &(*func_ptr).sig,
            (*func_ptr).fn_type,
        )
    };
    // `call_user_func*()` executes the resolved callback through this detached
    // boundary. Consume its discarded-result marker at the first user
    // callable; internal trampolines leave it published for a nested wrapper.
    let detached_return_discarded = user_callee
        .is_some()
        .then(|| eg.take_detached_return_discarded())
        .unwrap_or(false);
    if user_callee.is_some_and(|user| user.common.plan.has_deprecated_attribute()) {
        let source_override = trace_origin
            .as_ref()
            .map(|(file, line, _)| (file.as_str(), *line));
        report_deprecated_user_call(
            eg,
            saved_execute_data,
            func_ptr,
            None,
            source_override,
        )?;
        if eg.exception.is_some() {
            return Ok((Value::null(), None));
        }
    }
    if detached_return_discarded
        && user_callee.is_some_and(|user| user.common.plan.has_no_discard_attribute())
    {
        let source_override = trace_origin
            .as_ref()
            .map(|(file, line, _)| (file.as_str(), *line));
        report_no_discard_user_call(
            eg,
            saved_execute_data,
            user_callee.expect("detached NoDiscard marker belongs to a user function"),
            None,
            source_override,
        )?;
        if eg.exception.is_some() {
            return Ok((Value::null(), None));
        }
    }
    // Detached callback entries (Iterator, ArrayAccess, array_* callbacks, ...)
    // bypass DoFcall. Publish the suspended caller's current global-scope CVs
    // before a user callback that may execute `global $name`; otherwise the
    // callback can bind to an older ExecutorGlobals snapshot.
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
    let trace_caller = if logical_caller.is_null() {
        saved_execute_data
    } else {
        logical_caller
    };
    if !logical_caller.is_null() {
        if trace_caller_at_current_site {
            eg.publish_detached_trace_caller_at_current_site(
                frame as usize,
                trace_caller as usize,
            );
        } else {
            eg.publish_detached_trace_caller(frame as usize, trace_caller as usize);
        }
    }
    let pending_argument_error = trace_origin
        .as_ref()
        .and_then(|(_, _, throwable)| *throwable);
    if let Some((file, line, _)) = trace_origin.as_ref() {
        eg.publish_detached_trace_origin(frame as usize, file.clone(), *line);
    }
    let mut return_value = Value::null();
    let mut trace_arguments = (user_callee.is_some()
        && this_offset == 0
        && capture_count == 0
        && public_num_args > signature.param_names.len())
        .then(|| Vec::with_capacity(num_args));

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
            if let Some(trace_arguments) = trace_arguments.as_mut() {
                trace_arguments.push(arg.clone());
            }
            callback_arg_init(frame, i, arg);
        }
        debug_assert!(
            args.next().is_none(),
            "callback argument iterator longer than declared length"
        );
        if let Some(trace_arguments) = trace_arguments.take() {
            eg.function_arguments
                .insert(frame as usize, trace_arguments);
        }

        if called_scope_class_id != 0 {
            publish_late_static_call_class_id(eg, frame, called_scope_class_id);
        }
        if let Some(storage) = closure_static_vars.clone() {
            eg.publish_closure_static_vars(frame as usize, storage);
        }
        initialize_bound_this_frame(frame, func_ptr, bound_this);

        if let Some(throwable) = pending_argument_error {
            let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
                .as_deref()
                .is_some_and(crate::stdlib::ini_boolean);
            let trace_options = if ignore_arguments { 2 } else { 0 };
            let trace = crate::stdlib::collect_debug_backtrace(
                frame,
                trace_options,
                0,
                eg,
                true,
            );
            let function = Function::from_common_ptr(func_ptr);
            let location = if function.fn_type() == FunctionType::User {
                let op_array = &function.as_user().op_array;
                op_array
                    .declaration_line()
                    .filter(|_| !op_array.source_file.is_empty())
                    .map(|line| (op_array.source_file.to_string(), line))
            } else {
                trace_origin
                    .as_ref()
                    .map(|(file, line, _)| (file.clone(), *line))
            };
            if let Some((file, line)) = location
                && let Some(mut object) = throwable.as_object_mut()
            {
                object.set_property("file", Value::string(file));
                object.set_property("line", Value::long(line as i64));
                object.set_property("trace", Value::array(trace));
            }
            eg.discard_detached_trace_caller(frame as usize);
            cleanup_frame_slots(frame);
            pop_vm_call_frame(eg, frame);
            return Ok((Value::null(), None));
        }

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

            let mut arguments = Vec::with_capacity(user.op_array.num_cvs as usize);
            for index in 0..user.op_array.num_cvs {
                let value = (*frame).cv(index);
                arguments.push(value.clone_closure_capture());
            }
            let mut generator = Generator::new(
                func_ptr,
                arguments,
                user.op_array.num_cvs,
                user.op_array.num_temps,
            );
            generator.trace_num_args = Value::long(public_num_args as i64);
            generator.called_scope_class_id = called_scope_class_id;
            generator.closure_static_vars = closure_static_vars.clone();
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
            eg.discard_detached_trace_caller(frame as usize);
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
    unsafe {
        // Detached callbacks deliberately keep prev_execute_data null so a
        // Return opcode exits execute_ex instead of resuming the suspended
        // caller. If this frame created the throwable, temporarily reconnect
        // it before cleanup so PHP's stored trace still contains the callback
        // and its real call site. A non-empty trace belongs to a deeper frame
        // and must retain that immutable creation snapshot.
        let needs_detached_trace = callback_threw
            && !trace_caller.is_null()
            && eg.exception.as_ref().is_some_and(|exception| {
                exception.as_object().is_some_and(|object| {
                    object
                        .get_property("file")
                        .and_then(Value::as_str)
                        .is_some_and(|file| !file.is_empty())
                        && object
                            .get_property("line")
                            .and_then(Value::as_long)
                            .is_some_and(|line| line > 0)
                        && object
                            .get_property("trace")
                            .and_then(Value::as_array)
                            .is_none_or(PhpArray::is_empty)
                })
        });
        if needs_detached_trace {
            let caller_is_user = !(*trace_caller).func.is_null()
                && (*(*trace_caller).func).fn_type == FunctionType::User;
            let caller_opline = caller_is_user.then(|| (*trace_caller).opline);
            let advanced_caller = caller_opline.is_some_and(|opline| {
                let caller_op_array = (*trace_caller).op_array();
                let caller_index = opline.offset_from(caller_op_array.instructions.as_ptr());
                usize::try_from(caller_index)
                    .ok()
                    .filter(|index| *index < caller_op_array.instructions.len())
                    .is_some()
            });
            if advanced_caller {
                (*trace_caller).opline = caller_opline.unwrap().add(1);
            }
            (*frame).prev_execute_data = trace_caller;
            let trace = crate::stdlib::collect_debug_backtrace(frame, 0, 0, eg, true);
            (*frame).prev_execute_data = std::ptr::null_mut();
            if advanced_caller {
                (*trace_caller).opline = caller_opline.unwrap();
            }
            if let Some(mut exception) = eg.exception.as_ref().and_then(Value::as_object_mut) {
                exception.set_property("trace", Value::array(trace));
            }
        }
        eg.discard_detached_trace_caller(frame as usize);
        eg.current_execute_data.set(saved_execute_data);
        cleanup_frame_slots(frame);
    }
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

/// Snapshot a pending detached user call for an argument TypeError raised
/// before its body can execute. Attribute construction uses the declaration
/// site as its synthetic call origin while retaining the internal trampoline
/// and public arguments in the immutable Throwable trace.
pub(crate) fn attach_detached_argument_type_error_origin<I>(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
    call_file: &str,
    call_line: usize,
    throwable: &Value,
) -> Result<(), VmError>
where
    I: Iterator<Item = Value>,
{
    let _ = call_function_value_iter::<_, false>(
        eg,
        func_ptr,
        num_args,
        args,
        0,
        None,
        0,
        None,
        None,
        logical_caller,
        false,
        Some((call_file.to_string(), call_line, Some(throwable))),
    )?;
    Ok(())
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
        None,
        std::ptr::null_mut(),
        false,
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

enum GeneratorFrameInput {
    Send(Value),
    Throw(Value),
    SyntheticThrow(Value),
    Propagate(Value),
    YieldFromReturn(Value),
}

enum GeneratorPropagation {
    Yielded,
    Completed,
    Threw(Value, bool),
}

enum GeneratorFrameOutcome {
    Advanced,
    Threw(Value, bool),
}

struct ActiveGeneratorChain {
    indexed: Option<std::collections::HashSet<usize>>,
}

impl ActiveGeneratorChain {
    const INDEX_THRESHOLD: usize = 32;

    fn new() -> Self {
        Self { indexed: None }
    }

    fn contains(
        &self,
        current: &crate::vm::generator::GeneratorRef,
        parents: &[crate::vm::generator::GeneratorRef],
        candidate: &crate::vm::generator::GeneratorRef,
    ) -> bool {
        let candidate = generator_identity(candidate);
        if let Some(indexed) = &self.indexed {
            return indexed.contains(&candidate);
        }
        generator_identity(current) == candidate
            || parents
                .iter()
                .any(|parent| generator_identity(parent) == candidate)
    }

    fn record_descent(
        &mut self,
        current: &crate::vm::generator::GeneratorRef,
        parents: &[crate::vm::generator::GeneratorRef],
    ) {
        if let Some(indexed) = &mut self.indexed {
            indexed.insert(generator_identity(current));
            return;
        }
        if parents.len() < Self::INDEX_THRESHOLD {
            return;
        }

        let mut indexed = std::collections::HashSet::with_capacity(parents.len() + 1);
        indexed.extend(parents.iter().map(generator_identity));
        indexed.insert(generator_identity(current));
        self.indexed = Some(indexed);
    }

    fn record_ascent(&mut self, child: &crate::vm::generator::GeneratorRef) {
        if let Some(indexed) = &mut self.indexed {
            indexed.remove(&generator_identity(child));
        }
    }
}

/// Resume a generator: set up frame, copy state, execute until yield/return.
/// The generator's state is updated in place and detached exceptions are
/// returned explicitly rather than left in the executor sidecar.
pub(crate) fn resume_generator(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
) -> Result<GeneratorResumeOutcome, VmError> {
    resume_generator_with_input(eg, gen_ref, send_value, None)
}

/// Resume a generator by throwing an exception at its current suspension
/// point. The caller is responsible for priming a newly-created generator.
pub(crate) fn throw_into_generator(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    exception: Value,
) -> Result<GeneratorResumeOutcome, VmError> {
    resume_generator_with_input(eg, gen_ref, Value::null(), Some(exception))
}

fn resume_generator_with_input(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
    injected_exception: Option<Value>,
) -> Result<GeneratorResumeOutcome, VmError> {
    use crate::vm::generator::GeneratorState;

    match gen_ref.borrow().state {
        GeneratorState::Completed => return Ok(GeneratorResumeOutcome::Advanced),
        GeneratorState::Running => {
            return Err(VmError::Fatal(
                "Cannot resume an already running generator".into(),
            ));
        }
        GeneratorState::Suspended | GeneratorState::Created => {}
    }

    if gen_ref.borrow().delegate.is_some() {
        return resume_generator_delegation(
            eg,
            gen_ref,
            injected_exception.map_or(
                GeneratorFrameInput::Send(send_value),
                GeneratorFrameInput::Throw,
            ),
            false,
        );
    }

    let (frame, saved_execute_data) = materialize_generator_frame(eg, gen_ref);
    restore_yield_send_value(frame, gen_ref, send_value);
    execute_resumed_generator_frame(
        eg,
        gen_ref,
        frame,
        saved_execute_data,
        injected_exception,
        false,
        false,
        true,
    )
}

#[cold]
#[inline(never)]
fn resume_generator_delegation(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    mut input: GeneratorFrameInput,
    mut fresh_execution: bool,
) -> Result<GeneratorResumeOutcome, VmError> {
    use crate::vm::generator::{GeneratorRef, GeneratorState, YieldFromDelegate};

    let mut current = gen_ref.clone();
    let mut parents: Vec<GeneratorRef> = Vec::new();
    let mut active_delegates = ActiveGeneratorChain::new();
    let mut propagation: Option<GeneratorPropagation> = None;

    loop {
        if let Some(outcome) = propagation.take() {
            match outcome {
                GeneratorPropagation::Yielded => {
                    let mut child = current;
                    while let Some(parent) = parents.pop() {
                        let (value, key) = {
                            let child = child.borrow();
                            (child.value.clone(), child.key.clone())
                        };
                        {
                            let mut parent_data = parent.borrow_mut();
                            parent_data.value = value;
                            parent_data.key = key;
                            parent_data.state = GeneratorState::Suspended;
                        }
                        child = parent;
                    }
                    return Ok(GeneratorResumeOutcome::Advanced);
                }
                GeneratorPropagation::Completed => {
                    let Some(parent) = parents.pop() else {
                        return Ok(GeneratorResumeOutcome::Advanced);
                    };
                    let (has_returned, return_value) = {
                        let current = current.borrow();
                        (current.has_returned, current.return_value.clone())
                    };
                    active_delegates.record_ascent(&current);
                    let forwards_return = matches!(
                        parent.borrow().delegate.as_ref(),
                        Some(YieldFromDelegate::Generator(
                            _,
                            crate::vm::generator::YieldFromGeneratorMode::Direct
                        ))
                    );
                    {
                        let mut parent_data = parent.borrow_mut();
                        parent_data.delegate = None;
                        if has_returned {
                            parent_data.ip_offset += 1;
                        }
                    }
                    current = parent;
                    input = if has_returned {
                        GeneratorFrameInput::YieldFromReturn(if forwards_return {
                            return_value
                        } else {
                            Value::null()
                        })
                    } else if fresh_execution {
                        GeneratorFrameInput::SyntheticThrow(make_error_value(
                            "Error",
                            "Generator passed to yield from was aborted without proper return and is unable to continue",
                        ))
                    } else {
                        GeneratorFrameInput::SyntheticThrow(make_error_value(
                            "ClosedGeneratorException",
                            "Generator yielded from aborted, no return value available",
                        ))
                    };
                    fresh_execution = false;
                    continue;
                }
                GeneratorPropagation::Threw(exception, extend_trace) => {
                    let Some(parent) = parents.pop() else {
                        return Ok(GeneratorResumeOutcome::Threw(exception));
                    };
                    active_delegates.record_ascent(&current);
                    parent.borrow_mut().delegate = None;
                    current = parent;
                    input = if extend_trace {
                        GeneratorFrameInput::Propagate(exception)
                    } else {
                        GeneratorFrameInput::Throw(exception)
                    };
                    fresh_execution = false;
                    continue;
                }
            }
        }

        if fresh_execution {
            let state = current.borrow().state;
            match state {
                GeneratorState::Completed => {
                    propagation = Some(GeneratorPropagation::Completed);
                    continue;
                }
                GeneratorState::Suspended => {
                    let delegate = {
                        let current_data = current.borrow();
                        match current_data.delegate.as_ref() {
                            Some(YieldFromDelegate::Generator(delegate, _)) => {
                                Some(delegate.clone())
                            }
                            Some(YieldFromDelegate::Array(_, _))
                            | Some(YieldFromDelegate::Iterator(_))
                            | None => None,
                        }
                    };
                    let Some(delegate) = delegate else {
                        propagation = Some(GeneratorPropagation::Yielded);
                        continue;
                    };
                    if active_delegates.contains(&current, &parents, &delegate) {
                        current.borrow_mut().delegate = None;
                        let error = make_error_value(
                            "Error",
                            "Impossible to yield from the Generator being currently run",
                        );
                        match execute_generator_frame_input(
                            eg,
                            &current,
                            GeneratorFrameInput::SyntheticThrow(error),
                        )? {
                            GeneratorFrameOutcome::Advanced => {
                                fresh_execution = true;
                            }
                            GeneratorFrameOutcome::Threw(exception, extend_trace) => {
                                propagation = Some(GeneratorPropagation::Threw(
                                    exception,
                                    extend_trace,
                                ));
                            }
                        }
                        continue;
                    }
                    let delegate_state = delegate.borrow().state;
                    match delegate_state {
                        GeneratorState::Created => {
                            parents.push(current);
                            current = delegate;
                            active_delegates.record_descent(&current, &parents);
                            input = GeneratorFrameInput::Send(Value::null());
                            fresh_execution = false;
                            continue;
                        }
                        GeneratorState::Completed => {
                            parents.push(current);
                            current = delegate;
                            active_delegates.record_descent(&current, &parents);
                            propagation = Some(GeneratorPropagation::Completed);
                            continue;
                        }
                        GeneratorState::Suspended => {
                            parents.push(current);
                            current = delegate;
                            active_delegates.record_descent(&current, &parents);
                            fresh_execution = true;
                            continue;
                        }
                        GeneratorState::Running => {
                            return Err(VmError::Fatal(
                                "Cannot resume an already running generator".into(),
                            ));
                        }
                    }
                }
                GeneratorState::Running => {
                    return Err(VmError::Fatal(
                        "Generator resume returned without yielding or completing".into(),
                    ));
                }
                GeneratorState::Created => {
                    return Err(VmError::Fatal(
                        "Generator resume left the generator unstarted".into(),
                    ));
                }
            }
        }

        let state = current.borrow().state;
        match state {
            GeneratorState::Completed => {
                propagation = Some(GeneratorPropagation::Completed);
                continue;
            }
            GeneratorState::Running => {
                return Err(VmError::Fatal(
                    "Cannot resume an already running generator".into(),
                ));
            }
            GeneratorState::Suspended | GeneratorState::Created => {}
        }

        let generator_delegate = {
            let current_data = current.borrow();
            match current_data.delegate.as_ref() {
                Some(YieldFromDelegate::Generator(delegate, mode)) => {
                    Some((delegate.clone(), *mode))
                }
                Some(YieldFromDelegate::Array(_, _))
                | Some(YieldFromDelegate::Iterator(_))
                | None => None,
            }
        };
        if let Some((delegate, mode)) = generator_delegate {
            if active_delegates.contains(&current, &parents, &delegate) {
                current.borrow_mut().delegate = None;
                input = GeneratorFrameInput::SyntheticThrow(make_error_value(
                    "Error",
                    "Impossible to yield from the Generator being currently run",
                ));
                continue;
            }
            if mode == crate::vm::generator::YieldFromGeneratorMode::Traversable {
                if matches!(
                    &input,
                    GeneratorFrameInput::Throw(_)
                        | GeneratorFrameInput::SyntheticThrow(_)
                        | GeneratorFrameInput::Propagate(_)
                ) {
                    current.borrow_mut().delegate = None;
                    drop(delegate);
                    let frame_input = std::mem::replace(
                        &mut input,
                        GeneratorFrameInput::Send(Value::null()),
                    );
                    match execute_generator_frame_input(eg, &current, frame_input)? {
                        GeneratorFrameOutcome::Advanced => fresh_execution = true,
                        GeneratorFrameOutcome::Threw(exception, extend_trace) => {
                            propagation = Some(GeneratorPropagation::Threw(
                                exception,
                                extend_trace,
                            ));
                        }
                    }
                    continue;
                }
                // Iterator::next() advances a Generator-backed Traversable
                // with null; Generator::send() payloads are not forwarded
                // through an IteratorAggregate boundary.
                input = GeneratorFrameInput::Send(Value::null());
            }
            if delegate.borrow().rewindable {
                delegate.borrow_mut().rewindable = false;
            }
            parents.push(current);
            current = delegate;
            active_delegates.record_descent(&current, &parents);
            fresh_execution = false;
            continue;
        }

        let array_delegate = matches!(
            current.borrow().delegate,
            Some(YieldFromDelegate::Array(_, _))
        );
        if array_delegate {
            let delegate = current
                .borrow_mut()
                .delegate
                .take()
                .expect("array delegation disappeared");
            let YieldFromDelegate::Array(entries, position) = delegate else {
                unreachable!();
            };
            match std::mem::replace(
                &mut input,
                GeneratorFrameInput::Send(Value::null()),
            ) {
                frame_input @ (GeneratorFrameInput::Throw(_)
                | GeneratorFrameInput::SyntheticThrow(_)
                | GeneratorFrameInput::Propagate(_)) => {
                    drop(entries);
                    match execute_generator_frame_input(eg, &current, frame_input)? {
                        GeneratorFrameOutcome::Advanced => fresh_execution = true,
                        GeneratorFrameOutcome::Threw(exception, extend_trace) => {
                            propagation = Some(GeneratorPropagation::Threw(
                                exception,
                                extend_trace,
                            ));
                        }
                    }
                }
                GeneratorFrameInput::Send(_) | GeneratorFrameInput::YieldFromReturn(_) => {
                    if position >= entries.len() {
                        current.borrow_mut().ip_offset += 1;
                        match execute_generator_frame_input(
                            eg,
                            &current,
                            GeneratorFrameInput::YieldFromReturn(Value::null()),
                        )? {
                            GeneratorFrameOutcome::Advanced => fresh_execution = true,
                            GeneratorFrameOutcome::Threw(exception, extend_trace) => {
                                propagation = Some(GeneratorPropagation::Threw(
                                    exception,
                                    extend_trace,
                                ));
                            }
                        }
                    } else {
                        let (value, key) = {
                            let (key, value) = &entries[position];
                            let key = match key {
                                crate::value::ArrayKey::Int(key) => Value::long(*key),
                                crate::value::ArrayKey::String(key) => Value::string(key),
                            };
                            (value.clone(), key)
                        };
                        {
                            let mut current_data = current.borrow_mut();
                            current_data.value = value;
                            current_data.key = key;
                            current_data.delegate = Some(YieldFromDelegate::Array(
                                entries,
                                position + 1,
                            ));
                            current_data.state = GeneratorState::Suspended;
                        }
                        propagation = Some(GeneratorPropagation::Yielded);
                    }
                }
            }
            continue;
        }

        let iterator_delegate = matches!(
            current.borrow().delegate,
            Some(YieldFromDelegate::Iterator(_))
        );
        if iterator_delegate {
            let delegate = current
                .borrow_mut()
                .delegate
                .take()
                .expect("iterator delegation disappeared");
            let YieldFromDelegate::Iterator(iterator) = delegate else {
                unreachable!();
            };
            match std::mem::replace(
                &mut input,
                GeneratorFrameInput::Send(Value::null()),
            ) {
                frame_input @ (GeneratorFrameInput::Throw(_)
                | GeneratorFrameInput::SyntheticThrow(_)
                | GeneratorFrameInput::Propagate(_)) => {
                    match execute_generator_frame_input(eg, &current, frame_input)? {
                        GeneratorFrameOutcome::Advanced => fresh_execution = true,
                        GeneratorFrameOutcome::Threw(exception, extend_trace) => {
                            propagation = Some(GeneratorPropagation::Threw(
                                exception,
                                extend_trace,
                            ));
                        }
                    }
                }
                GeneratorFrameInput::Send(_) | GeneratorFrameInput::YieldFromReturn(_) => {
                    let step = yield_from_iterator_step(eg, &iterator, false)?;
                    if let Some(exception) = eg.exception.take() {
                        match execute_generator_frame_input(
                            eg,
                            &current,
                            GeneratorFrameInput::Propagate(exception),
                        )? {
                            GeneratorFrameOutcome::Advanced => fresh_execution = true,
                            GeneratorFrameOutcome::Threw(exception, extend_trace) => {
                                propagation = Some(GeneratorPropagation::Threw(
                                    exception,
                                    extend_trace,
                                ));
                            }
                        }
                    } else if let Some((key, value)) = step {
                        {
                            let mut current_data = current.borrow_mut();
                            current_data.value = value;
                            current_data.key = key;
                            current_data.delegate = Some(YieldFromDelegate::Iterator(iterator));
                            current_data.state = GeneratorState::Suspended;
                        }
                        propagation = Some(GeneratorPropagation::Yielded);
                    } else {
                        current.borrow_mut().ip_offset += 1;
                        match execute_generator_frame_input(
                            eg,
                            &current,
                            GeneratorFrameInput::YieldFromReturn(Value::null()),
                        )? {
                            GeneratorFrameOutcome::Advanced => fresh_execution = true,
                            GeneratorFrameOutcome::Threw(exception, extend_trace) => {
                                propagation = Some(GeneratorPropagation::Threw(
                                    exception,
                                    extend_trace,
                                ));
                            }
                        }
                    }
                }
            }
            continue;
        }

        let frame_input = std::mem::replace(
            &mut input,
            GeneratorFrameInput::Send(Value::null()),
        );
        match execute_generator_frame_input(eg, &current, frame_input)? {
            GeneratorFrameOutcome::Advanced => fresh_execution = true,
            GeneratorFrameOutcome::Threw(exception, extend_trace) => {
                propagation = Some(GeneratorPropagation::Threw(exception, extend_trace));
            }
        }
    }
}

fn generator_identity(generator: &crate::vm::generator::GeneratorRef) -> usize {
    std::rc::Rc::as_ptr(generator) as usize
}

fn execute_generator_frame_input(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    input: GeneratorFrameInput,
) -> Result<GeneratorFrameOutcome, VmError> {
    let escaped_same_input_extends = match &input {
        GeneratorFrameInput::Throw(exception) => {
            Some((exception.object_identity(), false))
        }
        GeneratorFrameInput::SyntheticThrow(exception)
        | GeneratorFrameInput::Propagate(exception) => {
            Some((exception.object_identity(), true))
        }
        GeneratorFrameInput::Send(_) | GeneratorFrameInput::YieldFromReturn(_) => None,
    };
    let (frame, saved_execute_data) = materialize_generator_frame(eg, gen_ref);
    let (injected_exception, seed_injected_trace, extend_injected_trace) = match input {
        GeneratorFrameInput::Send(value) => {
            restore_yield_send_value(frame, gen_ref, value);
            (None, false, false)
        }
        GeneratorFrameInput::Throw(exception) => (Some(exception), false, false),
        GeneratorFrameInput::SyntheticThrow(exception) => (Some(exception), true, false),
        GeneratorFrameInput::Propagate(exception) => (Some(exception), false, true),
        GeneratorFrameInput::YieldFromReturn(value) => {
            restore_yield_from_result(frame, gen_ref, value);
            (None, false, false)
        }
    };
    let outcome = execute_resumed_generator_frame(
        eg,
        gen_ref,
        frame,
        saved_execute_data,
        injected_exception,
        seed_injected_trace,
        extend_injected_trace,
        false,
    )?;
    Ok(match outcome {
        GeneratorResumeOutcome::Advanced => GeneratorFrameOutcome::Advanced,
        GeneratorResumeOutcome::Threw(exception) => {
            let extend_trace = escaped_same_input_extends.map_or(true, |(identity, extends)| {
                exception.object_identity() != identity || extends
            });
            GeneratorFrameOutcome::Threw(exception, extend_trace)
        }
    })
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
    let saved_execute_data = eg.current_execute_data.get();
    let frame = eg.vm_stack.push_call_frame(
        func_ptr,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    let gen_data = gen_ref.borrow();
    publish_late_static_call_class_id(eg, frame, gen_data.called_scope_class_id);
    if let Some(storage) = gen_data.closure_static_vars.clone() {
        eg.publish_closure_static_vars(frame as usize, storage);
    }
    // SAFETY: push_call_frame returned this live compiler-sized generator
    // frame; every restored CV/TMP index comes from its retained snapshot and
    // ip_offset belongs to the same immutable generator op-array.
    unsafe {
        let user = &*(func_ptr as *const UserFunction);
        (*frame).return_value = std::ptr::null_mut();
        for (i, value) in gen_data.cv_values.iter().enumerate() {
            let slot = (*frame).cv_mut(i as u32);
            frame_restore_slot(frame, slot as *mut Value, value.clone_closure_capture());
        }
        for (i, value) in gen_data.tmp_values.iter().enumerate() {
            let slot = (*frame).tmp_mut(i as u32);
            frame_restore_slot(frame, slot as *mut Value, value.clone_closure_capture());
        }
        (*frame).opline = user
            .op_array
            .instructions
            .as_ptr()
            .add(gen_data.ip_offset)
    };
    drop(gen_data);
    (frame, saved_execute_data)
}

#[inline(always)]
fn restore_yield_send_value(
    frame: *mut ExecuteData,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
) {
    let gen_data = gen_ref.borrow();
    if gen_data.ip_offset == 0 {
        return;
    }
    // SAFETY: materialize_generator_frame created this live frame from the
    // same retained function and ip_offset snapshot. A yielding result slot
    // is therefore inside its compiler-sized TMP envelope.
    unsafe {
        let user = &*(gen_data.func as *const UserFunction);
        let yield_instruction = &user.op_array.instructions[gen_data.ip_offset - 1];
        if yield_instruction.opcode == crate::vm::opcode::OpCode::Yield
            && yield_instruction.result_type != OpType::Unused
        {
            let slot = (*frame).slot_mut(yield_instruction.result as u32);
            frame_restore_slot(frame, slot as *mut Value, send_value);
        }
    }
}

fn restore_yield_from_result(
    frame: *mut ExecuteData,
    gen_ref: &crate::vm::generator::GeneratorRef,
    value: Value,
) {
    let gen_data = gen_ref.borrow();
    // SAFETY: the retained yield-from instruction and result-slot index came
    // from this generator's immutable function and compiler-sized live frame.
    unsafe {
        let user = &*(gen_data.func as *const UserFunction);
        let yield_from_instruction = &user.op_array.instructions[gen_data.ip_offset - 1];
        if yield_from_instruction.result_type != OpType::Unused {
            let slot = (*frame).slot_mut(gen_data.yield_from_result_slot);
            frame_restore_slot(frame, slot as *mut Value, value);
        }
    }
}

fn generator_resume_continuation_trace(
    eg: &ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    frame: *mut ExecuteData,
    saved_execute_data: *mut ExecuteData,
) -> PhpArray {
    let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
        .as_deref()
        .is_some_and(crate::stdlib::ini_boolean);
    // SAFETY: frame is the live detached generator root and
    // saved_execute_data is the still-live internal caller retained by this
    // synchronous resume. Its predecessor is likewise live while the call-site
    // pointer is advanced for the trace snapshot and then restored.
    unsafe {
        let trace_num_args = gen_ref
            .borrow()
            .trace_num_args
            .as_long()
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or(0);
        let saved_num_args = (*frame).num_args;
        (*frame).num_args = trace_num_args;
        let internal_caller = (*saved_execute_data).prev_execute_data;
        let (caller_opline, can_advance) = if internal_caller.is_null() {
            (std::ptr::null(), false)
        } else {
            let caller_opline = (*internal_caller).opline;
            let caller_op_array = (*internal_caller).op_array();
            let caller_index = caller_opline.offset_from(caller_op_array.instructions.as_ptr());
            let can_advance = usize::try_from(caller_index)
                .ok()
                .filter(|index| *index < caller_op_array.instructions.len())
                .is_some();
            (caller_opline, can_advance)
        };
        if can_advance {
            (*internal_caller).opline = caller_opline.add(1);
        }
        (*frame).prev_execute_data = saved_execute_data;
        let trace = crate::stdlib::collect_debug_backtrace(
            frame,
            if ignore_arguments { 2 } else { 0 },
            0,
            eg,
            true,
        );
        (*frame).num_args = saved_num_args;
        (*frame).prev_execute_data = std::ptr::null_mut();
        if can_advance {
            (*internal_caller).opline = caller_opline;
        }
        trace
    }
}

fn extend_generator_delegation_trace(
    mut existing: Vec<Value>,
    continuation: &PhpArray,
    origin: Option<&(std::rc::Rc<String>, usize)>,
) -> PhpArray {
    let boundary = existing
        .iter()
        .position(|value| {
            value
                .as_array()
                .and_then(|entry| entry.get_str("class"))
                .and_then(Value::as_str)
                .is_some_and(|class| class.eq_ignore_ascii_case("Generator"))
        })
        .unwrap_or(existing.len());
    if let Some((source_file, line)) = origin
        && boundary > 0
        && let Some(entry) = existing
            .get_mut(boundary - 1)
            .and_then(Value::as_array_mut)
    {
        if entry.get_str("file").is_none() {
            entry.set_str("file", Value::shared_string(source_file.clone()));
        }
        if entry.get_str("line").is_none() {
            entry.set_str("line", Value::long(*line as i64));
        }
    }
    let parent_frame = continuation.values().next().cloned();
    let mut complete = PhpArray::new();
    for (index, value) in existing.into_iter().enumerate() {
        if index == boundary
            && let Some(parent_frame) = parent_frame.as_ref()
        {
            complete.push(parent_frame.clone());
        }
        complete.push(value);
    }
    if boundary == complete.len()
        && let Some(parent_frame) = parent_frame
    {
        complete.push(parent_frame);
    }
    complete
}

#[cold]
#[inline(never)]
fn complete_escaped_generator_trace(
    exception: &Value,
    injected_exception_identity: Option<usize>,
    eg: &ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    frame: *mut ExecuteData,
    saved_execute_data: *mut ExecuteData,
) {
    if saved_execute_data.is_null()
        || exception.object_identity() == injected_exception_identity
    {
        return;
    }
    let existing_trace = exception
        .as_object()
        .and_then(|object| {
            object
                .get_property("trace")
                .and_then(Value::as_array)
                .map(|trace| trace.values().cloned().collect::<Vec<_>>())
        })
        .unwrap_or_default();
    let continuation =
        generator_resume_continuation_trace(eg, gen_ref, frame, saved_execute_data);
    let trace = if existing_trace.is_empty() {
        continuation
    } else {
        let mut complete = PhpArray::new();
        for value in existing_trace {
            complete.push(value);
        }
        for value in continuation.values() {
            complete.push(value.clone());
        }
        complete
    };
    if let Some(mut object) = exception.as_object_mut() {
        object.set_property("trace", Value::array(trace));
    }
}

#[cold]
#[inline(never)]
fn prepare_injected_generator_exception(
    exception: &Value,
    seed_trace: bool,
    extend_trace: bool,
    eg: &ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    frame: *mut ExecuteData,
    saved_execute_data: *mut ExecuteData,
) {
    let (op_array, instruction_index) = unsafe {
        let op_array = (*frame).op_array();
        let instruction_index = (*frame)
            .opline
            .offset_from(op_array.instructions.as_ptr()) as usize;
        (op_array, instruction_index)
    };
    attach_throwable_origin(exception, eg, frame, op_array, instruction_index);
    let origin = op_array
        .source_line(instruction_index)
        .map(|line| (op_array.source_file.clone(), line));
    let existing_trace = exception
        .as_object()
        .and_then(|object| {
            object
                .get_property("trace")
                .and_then(Value::as_array)
                .map(|trace| trace.values().cloned().collect::<Vec<_>>())
        })
        .unwrap_or_default();
    if saved_execute_data.is_null() || !((seed_trace && existing_trace.is_empty()) || extend_trace)
    {
        return;
    }
    let continuation =
        generator_resume_continuation_trace(eg, gen_ref, frame, saved_execute_data);
    let trace = if existing_trace.is_empty() {
        continuation
    } else {
        extend_generator_delegation_trace(existing_trace, &continuation, origin.as_ref())
    };
    if let Some(mut object) = exception.as_object_mut() {
        object.set_property("trace", Value::array(trace));
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
    seed_injected_trace: bool,
    extend_injected_trace: bool,
    follow_delegation: bool,
) -> Result<GeneratorResumeOutcome, VmError> {
    let saved_active = eg.active_generator.take();
    // A caller executing `finally` may already carry an exception that must
    // stay invisible to the detached generator. Normal advancement restores
    // it; a new escaped exception or VM failure supersedes it.
    let saved_exception = eg.exception.take();
    eg.active_generator = Some(gen_ref.clone());
    activate_generator_generic_context(eg, gen_ref, frame);
    eg.current_execute_data.set(frame);

    let injected_exception_identity = injected_exception
        .as_ref()
        .and_then(Value::object_identity);
    let result = if let Some(exception) = injected_exception {
        prepare_injected_generator_exception(
            &exception,
            seed_injected_trace,
            extend_injected_trace,
            eg,
            gen_ref,
            frame,
            saved_execute_data,
        );
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

    if let Some(exception) = escaped_exception.as_ref() {
        complete_escaped_generator_trace(
            exception,
            injected_exception_identity,
            eg,
            gen_ref,
            frame,
            saved_execute_data,
        );
    }

    cleanup_detached_frame_chain(eg, frame, false)?;
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
    let generator = gen_ref.borrow();
    if generator.state == crate::vm::generator::GeneratorState::Running {
        drop(generator);
        close_failed_generator(gen_ref);
        return Err(VmError::Fatal(
            "Generator resume returned without yielding or completing".into(),
        ));
    }
    let delegated = matches!(
        generator.delegate.as_ref(),
        Some(crate::vm::generator::YieldFromDelegate::Generator(_, _))
    );
    drop(generator);
    eg.exception = saved_exception;
    if follow_delegation && delegated {
        // The direct frame reached a new generator delegation. Its outer
        // frame has already unwound, so only this uncommon transition enters
        // the explicit delegation stack. Slow-path frame execution passes
        // false and lets its existing heap stack absorb further descendants
        // without nesting another Rust call.
        resume_generator_delegation(
            eg,
            gen_ref,
            GeneratorFrameInput::Send(Value::null()),
            true,
        )
    } else {
        Ok(GeneratorResumeOutcome::Advanced)
    }
}

fn close_failed_generator(gen_ref: &crate::vm::generator::GeneratorRef) {
    let mut generator = gen_ref.borrow_mut();
    generator.state = crate::vm::generator::GeneratorState::Completed;
    generator.value = Value::null();
    generator.key = Value::null();
    generator.delegate = None;
    generator.cv_values.clear();
    generator.tmp_values.clear();
}

/// A detached generator owns every frame above and including `root`. Normal
/// yield/return leaves the root allocated; an unhandled exception can also
/// leave nested callees above it. Reclaim the complete chain exactly once.
/// Release every live activation owned by one detached VM stack.
///
/// Fiber completion additionally runs PHP object destructors before each
/// frame is retired. Keep an already escaping exception outside destructor
/// dispatch so a destructor failure can replace it without making the old
/// exception visible to user code during `__destruct()`.
pub(crate) fn cleanup_detached_frame_chain(
    eg: &mut ExecutorGlobals,
    root: *mut ExecuteData,
    run_destructors: bool,
) -> Result<(), VmError> {
    let mut pending_exception = eg.exception.take();
    let mut cleanup_error = None;
    let mut frame = eg.current_execute_data.get();
    while !frame.is_null() {
        let previous = unsafe { (*frame).prev_execute_data };
        if run_destructors {
            loop {
                if let Err(error) = run_frame_destructors(eg, frame) {
                    cleanup_error = Some(error);
                }
                let Some(exception) = eg.exception.take() else {
                    break;
                };
                if let Some(previous) = pending_exception.as_ref() {
                    append_replaced_exception(&exception, previous, eg);
                }
                pending_exception = Some(exception);
                if cleanup_error.is_some() {
                    break;
                }
            }
        }
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
    eg.exception = pending_exception;
    if let Some(error) = cleanup_error {
        Err(error)
    } else {
        Ok(())
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
pub(crate) fn execute_coroutine_frame(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    boundary: *mut ExecuteData,
) -> Result<(), VmError> {
    let mut entry = frame;
    loop {
        execute_ex(eg, entry)?;
        if eg.exception.is_some() {
            return Ok(());
        }
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

/// Materialize the root activation owned by a suspended user callback. Fiber
/// and the opt-in coroutine runtime execute on detached VM stacks, so they
/// cannot use the ordinary caller-owned Send/DoFcall frame protocol.
pub(crate) fn initialize_suspended_callback_frame(
    eg: &mut ExecutorGlobals,
    callback: &crate::stdlib::ResolvedCallback,
    arguments: &[Value],
    return_value: *mut Value,
    logical_caller: *mut ExecuteData,
) -> Result<*mut ExecuteData, VmError> {
    // SAFETY: resolved callback descriptors and their immutable function
    // metadata are request-owned. The newly allocated compiler-sized frame
    // stays live on the active alternate VM stack until its Fiber owner
    // completes or explicitly cleans the initialization error path.
    unsafe {
    let common = &*callback.func_ptr;
    if common.fn_type != FunctionType::User {
        return Err(VmError::Fatal(
            "Suspended internal callbacks are not implemented".to_string(),
        ));
    }
    let user = &*(callback.func_ptr as *const UserFunction);
    if user.op_array.is_generator {
        return Err(VmError::Fatal(
            "Generator callbacks cannot be used as suspended roots".to_string(),
        ));
    }

    let public_num_args = arguments.len();
    let sequential = callback.prepend_args.len() + public_num_args;
    let capture_destination = common.sig.parameter_cv_count() as usize;
    let storage_num_args = sequential.max(capture_destination + callback.use_vars.len());
    let frame = eg.vm_stack.push_call_frame(
        callback.func_ptr,
        storage_num_args as u32,
        public_num_args as u32,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );

    // SAFETY: push_call_frame allocated every compiler-declared CV/TMP plus
    // the exact detached argument/capture envelope computed above.
    {
        (*frame).return_value = return_value;
        (*frame).opline = user.op_array.instructions.as_ptr();
        for index in 0..storage_num_args {
            callback_arg_init(frame, index, Value::undef());
        }
        for (index, value) in callback
            .prepend_args
            .iter()
            .chain(arguments)
            .enumerate()
        {
            frame_slot_set(frame, (*frame).cv_mut(index as u32), value.clone());
        }
        for (index, value) in callback.use_vars.iter().enumerate() {
            frame_slot_set(
                frame,
                (*frame).cv_mut((capture_destination + index) as u32),
                value.clone_closure_capture(),
            );
        }
        initialize_bound_this_frame(frame, callback.func_ptr, callback.bound_this.clone());
    }
    if callback.called_scope_class_id != 0 {
        publish_late_static_call_class_id(eg, frame, callback.called_scope_class_id);
    }
    if let Some(storage) = callback.closure_static_vars.clone() {
        eg.publish_closure_static_vars(frame as usize, storage);
    }
    eg.function_arguments
        .insert(frame as usize, arguments.to_vec());
    eg.publish_detached_trace_caller(frame as usize, logical_caller as usize);
    eg.publish_detached_trace_origin(frame as usize, "Unknown".to_string(), 0);
    eg.current_execute_data.set(frame);

    // Fiber callbacks are dispatched by the engine, so their public
    // arguments use PHP's weak-call coercion even when Fiber::start() was
    // written in a strict-types compilation unit.
    let callee_class = eg.declaring_class_of(callback.func_ptr).map(str::to_string);
    let mut argument_error = None;
    for (index, hint) in common.sig.param_type_hints.iter().enumerate() {
        if matches!(hint, ParamTypeHint::None) || index >= public_num_args {
            continue;
        }
        let cv_index = common.sig.param_cv_index(index as u32);
        let value = (*frame).cv(cv_index).dereferenced().clone();
        match prepare_call_argument(&value, hint, eg, false, callee_class.as_deref())? {
            CallArgumentPreparation::Exact => {}
            CallArgumentPreparation::Coerced(prepared) => {
                let slot = (*frame).cv_mut(cv_index) as *mut Value;
                if (*slot).is_reference() {
                    slot_set((*slot).as_ref_ptr(), prepared);
                } else {
                    frame_slot_set(frame, slot, prepared);
                }
            },
            CallArgumentPreparation::Invalid => {
                let parameter = common
                    .sig
                    .param_names
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("unknown");
                argument_error = Some(make_error_value(
                    "TypeError",
                    &format!(
                        "{}(): Argument #{} (${parameter}) must be of type {}, {} given",
                        displayed_function_name(eg, callback.func_ptr),
                        index + 1,
                        hint.diagnostic_display_name(),
                        declared_type_error_value_name(&value)
                    ),
                ));
                break;
            }
        }
    }
    if argument_error.is_none() && public_num_args < common.sig.required_num_args as usize {
        let required = common.sig.required_num_args;
        let relation = if common.sig.public_arity() > required {
            "at least"
        } else {
            "exactly"
        };
        argument_error = Some(make_error_value(
            "ArgumentCountError",
            &format!(
                "Too few arguments to function {}(), {public_num_args} passed and {relation} {required} expected",
                displayed_function_name(eg, callback.func_ptr)
            ),
        ));
    }
    if let Some(error) = argument_error {
        let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
            .as_deref()
            .is_some_and(crate::stdlib::ini_boolean);
        let trace_options = if ignore_arguments { 2 } else { 0 };
        let trace = crate::stdlib::collect_debug_backtrace(frame, trace_options, 0, eg, true);
        if let Some(mut object) = error.as_object_mut()
            && let Some(line) = user.op_array.declaration_line()
            && !user.op_array.source_file.is_empty()
        {
            object.set_property(
                "file",
                Value::shared_string(user.op_array.source_file.clone()),
            );
            object.set_property("line", Value::long(line as i64));
            object.set_property("trace", Value::array(trace));
        }
        eg.exception = Some(error);
    }
    Ok(frame)
    }
}

/// Inject `Fiber::throw()` at the suspended call boundary and return the frame
/// from which canonical execution should continue. An unhandled throwable is
/// left in ExecutorGlobals for the owning Fiber method to propagate.
pub(crate) fn inject_suspended_exception(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    exception: Value,
) -> Option<*mut ExecuteData> {
    match throw_in_frame(eg, frame, exception) {
        ThrowResult::Handled(new_frame, _) => {
            eg.current_execute_data.set(new_frame);
            Some(new_frame)
        }
        ThrowResult::Unhandled(exception) => {
            eg.exception = Some(exception);
            None
        }
    }
}
