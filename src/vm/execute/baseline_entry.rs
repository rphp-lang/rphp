// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[cold]
#[inline(never)]
fn finish_request_shutdown(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    shutdown_error: Option<VmError>,
) -> Result<(), VmError> {
    #[cfg(feature = "resource-lifetime")]
    let _resource_release_scope = eg.exception.as_ref()
        .map(|_| crate::resource_handle::ResourceReleaseScope::defer());
    eg.release_gc_destructor_owner(frame)?;
    if let Err(error) = run_request_cycle_destructors(eg, frame) {
        return Err(error);
    }
    eg.current_execute_data
        .set(unsafe { (*frame).prev_execute_data });
    if let Err(error) = run_shutdown_frame_destructors(eg, frame) {
        return Err(error);
    }
    unsafe { cleanup_frame_slots(frame) };
    // Releasing the root symbol table may expose a Fiber/Generator cycle that
    // was still legitimately live during the pre-frame object-store pass.
    // Close those newly unreachable contexts before static roots are retired,
    // matching Zend's request-shutdown ordering without requiring an explicit
    // userland gc_collect_cycles().
    if let Err(error) = run_request_cycle_destructors(eg, frame) {
        return Err(error);
    }
    if let Err(error) = run_request_static_destructors(eg, frame) {
        crate::value::end_object_handle_request();
        return Err(error);
    }
    shutdown_error.map_or(Ok(()), Err)
}

#[cold]
#[inline(never)]
fn finish_request_handler_shutdown(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Result<(), VmError> {
    // Active handlers must remain callable while class/function statics are
    // released and while final output-buffer callbacks run: either phase may
    // still throw. Retire handler-owned generators and objects only after both
    // final dispatch boundaries.
    let mut handler_roots = Vec::new();
    handler_roots.extend(eg.error_handler.take());
    for (handler, _) in eg.error_handler_stack.drain(..) {
        handler_roots.extend(handler);
    }
    handler_roots.extend(eg.exception_handler.take());
    for handler in eg.exception_handler_stack.drain(..) {
        handler_roots.extend(handler);
    }
    run_value_destructors(eg, &handler_roots, frame)?;
    drop(handler_roots);
    pop_vm_call_frame(eg, frame);
    Ok(())
}

#[cold]
fn attach_uncaught_string_conversion_replacement_trace(
    eg: &ExecutorGlobals,
    replacement: &Value,
    source: &Value,
) {
    let Some(mut replacement_object) = replacement.as_object_mut() else {
        return;
    };
    let trace_key = crate::runtime::throwable_private_property_key(eg, &replacement_object, "trace");
    let has_trace = replacement_object
        .get_property(&trace_key)
        .and_then(Value::as_array)
        .is_some_and(|trace| !trace.is_empty());
    if replacement_object.get_property("file").is_none() {
        replacement_object.set_property("file", Value::string(""));
    }
    if replacement_object.get_property("line").is_none() {
        replacement_object.set_property("line", Value::long(0));
    }
    let Some(source_object) = source.as_object() else {
        return;
    };
    let method = format!("{}::__tostring", source_object.class_name.to_ascii_lowercase());
    let function = eg.find_function(&method);
    let display_class = function
        .and_then(|function| eg.declaring_class_of(function))
        .unwrap_or(source_object.class_name.as_ref())
        .to_string();
    let (display_method, normalize_internal_method) = function.map_or_else(
        || ("__toString".to_string(), true),
        // SAFETY: find_function returned a live immutable table entry. Read
        // the UserFunction tail only after checking the common discriminant.
        |function| unsafe {
            if (*function).fn_type == FunctionType::User {
                let user = &*(function as *const UserFunction);
                (
                    user.op_array
                        .name
                        .rsplit_once("::")
                        .map_or("__toString", |(_, method)| method)
                        .to_string(),
                    false,
                )
            } else {
                ("__toString".to_string(), true)
            }
        },
    );
    if has_trace {
        let Some(mut trace) = replacement_object
            .get_property(&trace_key)
            .and_then(Value::as_array)
            .cloned()
        else {
            return;
        };
        if normalize_internal_method
            && let Some(mut first) = trace.get_value_at(0).cloned()
            && let Some(frame) = first.as_array_mut()
            && frame
                .get_str("function")
                .and_then(Value::as_str)
                .is_some_and(|function| function.eq_ignore_ascii_case("__tostring"))
        {
            frame.set_str("function", Value::string("__toString"));
            trace.set_int(0, first);
            replacement_object.set_property(&trace_key, Value::array(trace));
        }
        return;
    }
    let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
        .as_deref()
        .is_some_and(crate::stdlib::ini_boolean);
    let mut internal = PhpArray::with_hash_capacity(4);
    internal.set_str("function", Value::string(display_method));
    internal.set_str("class", Value::string(display_class));
    internal.set_str("type", Value::string("->"));
    if !ignore_arguments {
        internal.set_str("args", Value::array(PhpArray::new()));
    }
    let mut trace = PhpArray::with_packed_capacity(1);
    trace.push(Value::array(internal));
    replacement_object.set_property(&trace_key, Value::array(trace));
}

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
    // Request globals such as `$argv` and `$_SERVER` already live in the
    // global symbol table; the main scope's compiled variables are that table.
    if !eg.globals.is_empty() || !eg.jit_auto_globals.is_empty() {
        for (cv, name) in &main_func.op_array.main_scope_vars {
            eg.materialize_auto_global(name);
            if let Some(value) = eg.globals.get(name) {
                let binding = clone_scope_binding(value);
                // SAFETY: the frame was pushed above for exactly this op array,
                // whose main-scope CV indices come from the same compilation.
                unsafe {
                    let slot = (*frame).cv_mut(*cv) as *mut Value;
                    frame_slot_set(frame, slot, binding);
                }
            }
        }
    }
    eg.current_execute_data.set(frame);

    let mut execution = execute_ex(eg, frame);
    if execution.is_ok()
        && main_func.op_array.source_file.as_ref() != "Command line code"
        && eg.exception.is_some()
        && eg.exception_handler.is_some()
    {
        // The root frame stays live until the engine callback returns: PHP
        // runs the handler before main-scope destructors, and detached callback
        // traces still terminate at the synthetic `{main}` frame.
        eg.current_execute_data.set(frame);
        if let Err(error) =
            crate::stdlib::dispatch_pending_uncaught_exception_handlers(eg, frame)
        {
            execution = Err(error);
        }
    }
    let mut prepared_uncaught = None;
    if execution.is_ok()
        && let Some(mut effective) = eg.exception.take()
    {
        let parse_error = effective.as_object().is_some_and(|object| {
            object.class_name.as_ref().eq_ignore_ascii_case("ParseError")
        });
        if !parse_error {
            eg.current_execute_data.set(frame);
            let mut replaced_during_render = false;
            for _ in 0..16 {
                let rendered =
                    call_object_string_conversion_from_internal(eg, frame, &effective)?;
                if let Some(replacement) = eg.exception.take() {
                    attach_uncaught_string_conversion_replacement_trace(
                        eg,
                        &replacement,
                        &effective,
                    );
                    effective = replacement;
                    replaced_during_render = true;
                    continue;
                }
                if let Some(rendered) = rendered {
                    let rendered = rendered.as_str().unwrap_or_default();
                    let (file, line, type_error_definition_site) =
                        effective.as_object().map_or_else(
                        || ("[no active file]".to_string(), 0, false),
                        |object| {
                            let file = object
                                .get_property("file")
                                .and_then(Value::as_str)
                                .unwrap_or("[no active file]")
                                .to_string();
                            let line = object
                                .get_property("line")
                                .and_then(Value::as_long)
                                .unwrap_or(0);
                            let type_error_definition_site = object
                                .class_name
                                .as_ref()
                                .eq_ignore_ascii_case("TypeError")
                                && object
                                    .get_property("message")
                                    .and_then(Value::as_str)
                                    .is_some_and(|message| {
                                        message.contains(", called in ")
                                            && message.contains(" on line ")
                                    });
                            (file, line, type_error_definition_site)
                        },
                    );
                    let mut rendered = if replaced_during_render && file.is_empty() {
                        rendered.replacen(
                            &format!(" in :{line}\nStack trace:"),
                            &format!(" in [no active file]:{line}\nStack trace:"),
                            1,
                        )
                    } else {
                        rendered.to_string()
                    };
                    let canonical_definition =
                        format!(" and defined in {file}:{line}\nStack trace:");
                    if type_error_definition_site && !rendered.contains(&canonical_definition) {
                        rendered = rendered.replacen(
                            &format!(" in {file}:{line}\nStack trace:"),
                            &canonical_definition,
                            1,
                        );
                    }
                    prepared_uncaught = effective.object_identity().map(|identity| {
                        let thrown_file = if file.is_empty() {
                            if replaced_during_render {
                                "[no active file]"
                            } else {
                                "Unknown"
                            }
                        } else {
                            &file
                        };
                        let rendered = if rendered.contains("\nStack trace:")
                            || replaced_during_render
                            || (!file.is_empty() && file != "[no active file]")
                        {
                            format!(
                                "Uncaught {rendered}\n  thrown in {thrown_file} on line {line}"
                            )
                        } else {
                            format!("Uncaught {rendered}")
                        };
                        (identity, rendered)
                    });
                    break;
                }
                break;
            }
        }
        eg.exception = Some(effective);
    }
    if eg.exception.is_some() {
        run_exception_frame_generator_destructors(eg, frame)?;
    }
    if let Some(exception) = eg.exception.as_ref().cloned() {
        let parse_error = exception.as_object().is_some_and(|object| {
            object.class_name.as_ref().eq_ignore_ascii_case("ParseError")
        });
        if !parse_error {
            let rendered = prepared_uncaught
                .as_ref()
                .filter(|(identity, _)| exception.object_identity() == Some(*identity))
                .map(|(_, rendered)| rendered.clone())
                .unwrap_or_else(|| format_uncaught_throwable(eg, &exception));
            let (file, line) = exception.as_object().map_or_else(
                || ("Unknown".to_string(), 0),
                |object| {
                    (
                        object
                            .get_property("file")
                            .and_then(Value::as_str)
                            .filter(|file| !file.is_empty())
                            .unwrap_or("Unknown")
                            .to_string(),
                        object
                            .get_property("line")
                            .and_then(Value::as_long)
                            .unwrap_or(0)
                            .max(0) as usize,
                    )
                },
            );
            let last_error_suffix = format!(" in {file} on line {line}");
            let last_error_message = rendered
                .strip_suffix(&last_error_suffix)
                .unwrap_or(&rendered);
            eg.record_last_error(1, last_error_message, &file, line);
        }
        // Request-final callbacks and resource close hooks run before the CLI
        // publishes either exception class, but their output follows the
        // already prepared fatal diagnostic in PHP's observable byte order.
        eg.begin_post_fatal_output();
    }
    let pending_uncaught = eg.exception.take();
    let pending_uncaught_identity = pending_uncaught.as_ref().and_then(Value::object_identity);
    let mut shutdown_error = None;
    if execution.is_ok() && eg.shutdown_functions.is_some() {
        shutdown_error = crate::stdlib::run_shutdown_functions(eg, frame).err();
    }
    if eg.exception.is_none() {
        eg.exception = pending_uncaught;
    }
    if let Err(error) = execution {
        crate::value::end_object_handle_request();
        return Err(error);
    }
    if let Err(error) = finish_request_shutdown(eg, frame, shutdown_error) {
        let _ = finish_request_handler_shutdown(eg, frame);
        crate::value::end_object_handle_request();
        return Err(error);
    }

    let output_result = crate::stdlib::flush_all_output_buffers(eg);
    let handler_dispatch_result = if output_result.is_ok()
        && eg.exception.is_some()
        && eg.exception_handler.is_some()
        && eg.exception.as_ref().and_then(Value::object_identity) != pending_uncaught_identity
    {
        eg.current_execute_data.set(frame);
        crate::stdlib::dispatch_pending_uncaught_exception_handlers(eg, frame)
    } else {
        Ok(())
    };
    let handler_shutdown_result = finish_request_handler_shutdown(eg, frame);
    crate::value::end_object_handle_request();
    output_result?;
    handler_dispatch_result?;
    handler_shutdown_result?;

    // Check for uncaught exception that propagated through execute_ex.
    if let Some(exc) = eg.exception.take() {
        if exc.as_object().is_some_and(|object| {
            object.class_name.as_ref().eq_ignore_ascii_case("ParseError")
        }) {
            return Err(VmError::Parse(format_parse_error(&exc)));
        }
        if let Some((identity, rendered)) = prepared_uncaught
            && exc.object_identity() == Some(identity)
        {
            return Err(VmError::Fatal(rendered));
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
    format_throwable_chain(eg, thrown, true, None)
}

#[cold]
pub(crate) fn format_throwable_string_with_messages(
    eg: &ExecutorGlobals,
    thrown: &Value,
    messages: &HashMap<usize, String>,
) -> String {
    format_throwable_chain(eg, thrown, false, Some(messages))
}

#[cold]
fn format_throwable_chain(
    eg: &ExecutorGlobals,
    thrown: &Value,
    uncaught: bool,
    messages: Option<&HashMap<usize, String>>,
) -> String {
    struct Segment {
        class_name: String,
        message: String,
        location: Option<(String, i64, PhpArray)>,
    }

    fn snapshot(
        eg: &ExecutorGlobals,
        value: &Value,
        messages: Option<&HashMap<usize, String>>,
    ) -> Option<Segment> {
        let object = value.as_object()?;
        let class_name = object.class_name.to_string();
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        let message = value
            .object_identity()
            .and_then(|identity| messages.and_then(|messages| messages.get(&identity)))
            .cloned()
            .unwrap_or_else(|| {
                object
                    .get_property("message")
                    .map(Value::dereferenced)
                    .map(Value::echo_to_string)
                    .unwrap_or_default()
            });
        let location = object
            .get_property("file")
            .map(Value::dereferenced)
            .and_then(Value::as_str)
            .zip(
                object
                    .get_property("line")
                    .map(Value::dereferenced)
                    .and_then(Value::as_long),
            )
            .zip(
                object
                    .get_property(&trace_key)
                    .map(Value::dereferenced)
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
                    .dereferenced()
                    .as_object()
                    .is_some_and(|object| eg.class_is_a(&object.class_name, "Throwable"))
            })
            .cloned()
    }

    let Some(final_segment) = snapshot(eg, thrown, messages) else {
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
        let Some(segment) = snapshot(eg, &candidate, messages) else {
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
        rendered.push_str(if file.is_empty() { "Unknown" } else { &file });
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

/// Prepared proof for a pure Long callback that writes only its first
/// by-reference parameter. The compiled function allocation remains stable
/// for the request, while the owned plan is built once per array consumer.
pub(crate) struct ScalarLongReferenceMutationCallback {
    common: &'static FunctionCommon,
    plan: Box<crate::vm::function::ScalarLongFunctionPlan>,
}

#[inline]
pub(crate) fn prepare_scalar_long_reference_mutation_callback(
    func_ptr: *const FunctionCommon,
    capture_count: usize,
) -> Option<ScalarLongReferenceMutationCallback> {
    // SAFETY: callback resolution publishes only immutable registered function
    // descriptors, and the returned proof is consumed synchronously before
    // that request-owned allocation can be released.
    unsafe {
        if func_ptr.is_null() || (*func_ptr).fn_type != FunctionType::User {
            return None;
        }
        let common = &*func_ptr;
        let user = &*(func_ptr as *const UserFunction);
        let plan = crate::compiler::build_scalar_long_reference_mutation_plan(user, capture_count)?;
        Some(ScalarLongReferenceMutationCallback { common, plan })
    }
}

impl ScalarLongReferenceMutationCallback {
    #[inline(always)]
    pub(crate) fn evaluate_longs(&self, arguments: &[i64]) -> Option<i64> {
        if arguments.len() != self.plan.public_args as usize {
            return None;
        }
        let mut scalar_arguments = [0i64; 8];
        scalar_arguments[..arguments.len()].copy_from_slice(arguments);
        evaluate_scalar_long_plan(&self.plan, &scalar_arguments)
    }

    #[inline(always)]
    pub(crate) fn record_calls(&self, count: u64) {
        record_scalar_calls_bulk(self.common, count);
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
        None,
        0,
        None,
        None,
        std::ptr::null_mut(),
        false,
        false,
        None,
    )?;
    Ok(return_value)
}

/// Invoke an engine-dispatched callback whose logical caller can differ from
/// the currently active frame. Frame-unwind destructors use the caller of the
/// retiring activation so stored traces never retain a frame that is about to
/// be released.
fn call_function_iter_from_logical_caller<'a, I>(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
    internal_trace_origin: bool,
    logical_caller_at_current_site: bool,
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
        None,
        0,
        None,
        None,
        logical_caller,
        true,
        logical_caller_at_current_site,
        internal_trace_origin.then(|| ("Unknown".to_string(), 0, None, false)),
    )?;
    Ok(return_value)
}

/// Invoke a callback whose real internal caller is synchronously live, but do
/// not publish the detached-caller shortcut while the callback runs. If it
/// throws, cleanup reconnects the callback frame to the internal activation
/// and snapshots both frames; ordinary callback global synchronization still
/// starts from the active user frame rather than interpreting an internal
/// descriptor as an OpArray.
fn call_function_iter_from_live_internal_caller<'a, I>(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
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
        None,
        0,
        None,
        None,
        logical_caller,
        false,
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
        None,
        0,
        None,
        None,
        logical_caller,
        true,
        true,
        None,
    )?;
    Ok(return_value)
}

/// Invoke a registered internal function from a source opcode that already
/// owns the call boundary. The detached activation is linked to the source
/// frame only while its native handler runs, so diagnostics retain the
/// physical callsite without changing callback or nullsafe trace behavior.
pub(crate) fn call_internal_function_iter_from_current_site<'a, I>(
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
        None,
        0,
        None,
        None,
        logical_caller,
        false,
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
    closure_scope_class_id: Option<u32>,
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
        closure_scope_class_id,
        bound_this.cloned(),
        capture_count,
        closure_static_vars,
        None,
        std::ptr::null_mut(),
        false,
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
            None,
            0,
            None,
            None,
            std::ptr::null_mut(),
            false,
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
    closure_scope_class_id: Option<u32>,
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
        closure_scope_class_id,
        bound_this,
        capture_count,
        closure_static_vars,
        None,
        std::ptr::null_mut(),
        false,
        false,
        None,
    )?;
    Ok(return_value)
}

/// Owned callback entry that retains an internal function frame as the
/// logical caller without manufacturing a source origin for the callback
/// itself. Array walkers use this so a thrown callback records both the
/// callback and `array_walk*` frames, matching Zend's internal-call trace.
pub(crate) fn call_function_owned_iter_with_context_from<I>(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
    called_scope_class_id: u32,
    closure_scope_class_id: Option<u32>,
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
        closure_scope_class_id,
        bound_this,
        capture_count,
        closure_static_vars,
        None,
        logical_caller,
        true,
        false,
        None,
    )?;
    Ok(return_value)
}

/// Owned callback entry that may retain the internal caller for live nested
/// traces. Proven leaf callback bodies defer that link until a Throwable is
/// materialized, avoiding sidecar updates on every successful call.
pub(crate) fn call_function_owned_iter_with_context_from_mode<I>(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
    func_ptr: *const FunctionCommon,
    num_args: usize,
    args: I,
    called_scope_class_id: u32,
    closure_scope_class_id: Option<u32>,
    bound_this: Option<Value>,
    capture_count: usize,
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
    publish_live_trace_caller: bool,
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
        closure_scope_class_id,
        bound_this,
        capture_count,
        closure_static_vars,
        None,
        logical_caller,
        publish_live_trace_caller,
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
    closure_scope_class_id: Option<u32>,
    bound_this: Option<Value>,
    capture_count: usize,
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
    named_variadic: Vec<(String, Value)>,
    named_variadic_external_byte_keys: bool,
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
        closure_scope_class_id,
        bound_this,
        capture_count,
        closure_static_vars,
        Some((named_variadic, named_variadic_external_byte_keys)),
        std::ptr::null_mut(),
        false,
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
    closure_scope_class_id: Option<u32>,
    bound_this: Option<Value>,
    capture_count: usize,
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
    named_variadic: Vec<(String, Value)>,
    named_variadic_external_byte_keys: bool,
    trace_origin: (String, usize),
    pending_preentry_error: Option<&Value>,
    capture_generated_preentry_error: bool,
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
        closure_scope_class_id,
        bound_this,
        capture_count,
        closure_static_vars,
        Some((named_variadic, named_variadic_external_byte_keys)),
        logical_caller,
        true,
        false,
        Some((
            trace_origin.0,
            trace_origin.1,
            pending_preentry_error,
            capture_generated_preentry_error,
        )),
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
            None,
            0,
            None,
            None,
            std::ptr::null_mut(),
            false,
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
    closure_scope_class_id: Option<u32>,
    bound_this: Option<Value>,
    capture_count: usize,
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
        closure_scope_class_id,
        bound_this,
        capture_count,
        closure_static_vars,
        None,
        std::ptr::null_mut(),
        false,
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
    closure_scope_class_id: Option<u32>,
    bound_this: Option<Value>,
    capture_count: usize,
    closure_static_vars: Option<crate::value::ClosureStaticVars>,
    named_variadic: Option<(Vec<(String, Value)>, bool)>,
    logical_caller: *mut ExecuteData,
    publish_live_trace_caller: bool,
    trace_caller_at_current_site: bool,
    trace_origin: Option<(String, usize, Option<&Value>, bool)>,
) -> Result<(Value, Option<Value>), VmError>
where
    I: Iterator<Item = Value>,
{
    let saved_execute_data = eg.current_execute_data.get();
    // SAFETY: the detached-call boundary receives a resolved registered
    // function descriptor that remains live for the synchronous invocation.
    // SAFETY: exact-arity and deprecation metadata are read only after the
    // descriptor's internal kind has selected its registered InternalFunction
    // tail.
    let (
        user_callee,
        signature,
        function_type,
        exact_arity_diagnostics,
        internal_deprecation,
    ) = unsafe {
        let function_type = (*func_ptr).fn_type;
        let internal = (function_type == FunctionType::Internal)
            .then(|| &*(func_ptr as *const super::function::InternalFunction));
        (
            (function_type == FunctionType::User).then(|| &*(func_ptr as *const UserFunction)),
            &(*func_ptr).sig,
            function_type,
            internal.is_some_and(|function| function.exact_arity_diagnostics),
            internal.and_then(|function| function.deprecation),
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
            .map(|(file, line, _, _)| (file.as_str(), *line));
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
            .map(|(file, line, _, _)| (file.as_str(), *line));
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
    // bypass DoFcall. Publish the suspended request's current main-scope CVs
    // before a user callback that may execute `global $name` or access
    // `$GLOBALS`; otherwise a callback entered from inside another function
    // can bind to an older ExecutorGlobals snapshot.
    if let Some(user) = user_callee
        && user.op_array.may_access_globals
        && !saved_execute_data.is_null()
    {
        unsafe {
            let mut scope = saved_execute_data;
            let mut fallback = None;
            while !scope.is_null() {
                let frame = &mut *scope;
                // Internal activations (including explicit GC) carry only
                // FunctionCommon plus an internal tail, never a UserFunction
                // op-array. Skip them before inspecting lexical bindings.
                if frame.func.is_null()
                    || Function::from_common_ptr(frame.func).fn_type() != FunctionType::User
                {
                    scope = frame.prev_execute_data;
                    continue;
                }
                sync_dirty_globals_to_frame(eg, frame);
                let scope_op_array = frame.op_array();
                if fallback.is_none() && !scope_op_array.global_vars.is_empty() {
                    fallback = Some(scope);
                }
                if !scope_op_array.main_scope_vars.is_empty() {
                    for (cv, name) in &scope_op_array.main_scope_vars {
                        globals_set(
                            &mut eg.globals,
                            name,
                            clone_scope_binding(frame.cv(*cv)),
                        );
                    }
                    scope = std::ptr::null_mut();
                } else {
                    scope = frame.prev_execute_data;
                }
            }
            if let Some(scope) = fallback {
                let frame = &*scope;
                if frame.op_array().main_scope_vars.is_empty() {
                    for (cv, name) in &frame.op_array().global_vars {
                        globals_set(
                            &mut eg.globals,
                            name,
                            clone_scope_binding(frame.cv(*cv)),
                        );
                    }
                }
            }
        }
    }
    let this_offset = signature.this_offset as usize;
    let positional_public_num_args = num_args.saturating_sub(this_offset + capture_count);
    let public_num_args = positional_public_num_args
        .saturating_add(named_variadic.as_ref().map_or(0, |(named, _)| named.len()));
    let rejects_internal_named_variadic = function_type == FunctionType::Internal
        && signature.is_variadic
        && named_variadic
            .as_ref()
            .is_some_and(|(named, _)| !named.is_empty())
        && !crate::stdlib::internal_variadic_forwards_named_arguments(
            &displayed_function_name(eg, func_ptr),
        );
    let arity_num_args = if rejects_internal_named_variadic {
        positional_public_num_args
    } else {
        public_num_args
    };
    let supplied_preentry_error = trace_origin
        .as_ref()
        .and_then(|(_, _, throwable, _)| *throwable);
    let capture_generated_preentry_error = trace_origin
        .as_ref()
        .is_some_and(|(_, _, _, capture)| *capture);
    let mut generated_preentry_error = None;
    if supplied_preentry_error.is_none()
        && let Some(deprecation) = internal_deprecation
    {
        let source_override = trace_origin
            .as_ref()
            .map(|(file, line, _, _)| (file.as_str(), *line));
        report_deprecated_internal_call(
            eg,
            saved_execute_data,
            func_ptr,
            deprecation,
            source_override,
        )?;
        if eg.exception.is_some() {
            return Ok((Value::null(), None));
        }
    }
    if supplied_preentry_error.is_none()
        && arity_num_args < signature.required_num_args as usize
    {
        let common = unsafe { &*func_ptr };
        let required = signature.required_num_args;
        let relation = if common.fn_type == FunctionType::Internal {
            if exact_arity_diagnostics {
                "exactly"
            } else if signature.is_variadic || signature.public_arity() > required {
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
                "{name}() expects {relation} {required} {noun}, {arity_num_args} given"
            )
        } else {
            format!(
                "Too few arguments to function {name}(), {arity_num_args} passed and {relation} {required} expected"
            )
        };
        let error = make_error_value("ArgumentCountError", &message);
        eg.exception = Some(error.clone());
        if !capture_generated_preentry_error {
            return Ok((Value::null(), None));
        }
        generated_preentry_error = Some(error);
    }
    if supplied_preentry_error.is_none()
        && generated_preentry_error.is_none()
        && rejects_internal_named_variadic
    {
        let name = displayed_function_name(eg, func_ptr);
        let error = make_error_value(
            "ArgumentCountError",
            &format!("{name}() does not accept unknown named parameters"),
        );
        eg.exception = Some(error.clone());
        if !capture_generated_preentry_error {
            return Ok((Value::null(), None));
        }
        generated_preentry_error = Some(error);
    }
    if supplied_preentry_error.is_none()
        && generated_preentry_error.is_none()
        && function_type == FunctionType::Internal
        && !signature.is_variadic
        && public_num_args > signature.public_arity() as usize
    {
        let error = too_many_internal_arguments_error(
            eg,
            func_ptr,
            signature,
            public_num_args as u32,
            exact_arity_diagnostics,
        );
        eg.exception = Some(error.clone());
        if !capture_generated_preentry_error {
            return Ok((Value::null(), None));
        }
        generated_preentry_error = Some(error);
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
    if publish_live_trace_caller && !logical_caller.is_null() {
        if trace_caller_at_current_site {
            eg.publish_detached_trace_caller_at_current_site(
                frame as usize,
                trace_caller as usize,
            );
        } else {
            eg.publish_detached_trace_caller(frame as usize, trace_caller as usize);
        }
    }
    if let Some((file, line, _, _)) = trace_origin.as_ref() {
        eg.publish_detached_trace_origin(frame as usize, file.clone(), *line);
    }
    let mut return_value = Value::null();
    let mut generated_argument_type_error = None;
    let capture_source_start = num_args.saturating_sub(capture_count);
    let first_surplus_argument = signature.public_arity();
    let mut trace_arguments = (user_callee.is_some()
        && public_num_args > signature.param_names.len())
        .then(|| {
            eg.take_function_argument_buffer(
                positional_public_num_args.saturating_sub(first_surplus_argument as usize),
            )
        });

    // SAFETY: `frame` is a fresh compiler-sized activation. All argument and
    // variadic destinations are uninitialized slots described by `func_ptr`;
    // a non-null `trace_caller` is a live synchronous source activation.
    unsafe {
        // Most engine callbacks use weak internal coercion. Only a source
        // call_user_func_array() marker carries strictness into its detached
        // native-ZPP target without changing adjacent callback contracts.
        (*frame).set_detached_strict_call(
            !trace_caller.is_null() && (*trace_caller).is_detached_strict_call(),
        );
        (*frame).return_value = &mut return_value;
        // prev=null so Return exits execute_ex instead of continuing in caller

        // Initialize every CV slot described by the detached argument
        // envelope. Surplus non-variadic user values live only in the stable
        // tail snapshot; initialize their overlapping local slots directly as
        // Undef so cleanup metadata remains valid without a retain/drop cycle.
        let public_parameter_end = this_offset + signature.public_arity() as usize;
        for i in 0..num_args {
            let arg = args
                .next()
                .expect("callback argument iterator shorter than declared length");
            if let Some(trace_arguments) = trace_arguments.as_mut()
                && i >= public_parameter_end
                && i < capture_source_start
            {
                trace_arguments.push(arg.clone());
            }
            let needs_frame_value = user_callee.is_none()
                || i < public_parameter_end
                || (signature.is_variadic && i < capture_source_start)
                || i >= capture_source_start;
            callback_arg_init(
                frame,
                i,
                if needs_frame_value { arg } else { Value::undef() },
            );
        }
        debug_assert!(
            args.next().is_none(),
            "callback argument iterator longer than declared length"
        );
        if let Some(trace_arguments) = trace_arguments.take() {
            eg.publish_function_arguments(
                frame as usize,
                crate::runtime::FunctionArgumentSnapshot {
                    first: first_surplus_argument,
                    values: trace_arguments,
                },
            );
        }

        if called_scope_class_id != 0 {
            publish_late_static_call_class_id(eg, frame, called_scope_class_id);
        }
        if let Some(storage) = closure_static_vars.clone() {
            eg.publish_closure_static_vars(frame as usize, storage);
        }
        // Snapshot the trailing captures before the bound receiver lands:
        // with surplus public arguments a capture can sit in the closure's
        // `$this` CV, which the receiver write would otherwise clobber.
        let saved_captures = (capture_count != 0).then(|| {
            let start = this_offset + positional_public_num_args;
            (0..capture_count)
                .map(|index| (*frame).cv((start + index) as u32).clone_closure_capture())
                .collect::<Vec<_>>()
        });
        initialize_bound_this_frame(frame, func_ptr, bound_this, closure_scope_class_id);

        // Detached callback entry bypasses DoFcall, whose full path normally
        // materializes the variadic bucket. Internal handlers use the same ABI in
        // both entry modes, so pack their trailing public arguments here before
        // dispatching the handler.

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
            if let Some((named, external_byte_keys)) = named_variadic {
                if external_byte_keys {
                    variadic.promote_keys_to_external_storage();
                }
                for (name, value) in named {
                    if external_byte_keys {
                        variadic.set(ArrayKey::String(name), value);
                    } else {
                        variadic.set_str(&name, value);
                    }
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

        // Source opcodes validate user arguments while executing their Send*
        // sequence. Engine-dispatched callbacks enter here after that sequence,
        // so apply the same declared-type contract before the callback body.
        // Internal callback consumers are weak call sites in PHP: scalar
        // coercions are written back into the fresh frame, while class and
        // compound mismatches retain the internal caller in the Throwable
        // trace assembled below.
        if user_callee.is_some()
            && supplied_preentry_error.is_none()
            && generated_preentry_error.is_none()
        {
            let callee_class = lexical_class_name_for_frame(eg, frame);
            let fixed_arity = if signature.is_variadic {
                signature.public_arity() as usize
            } else {
                signature.param_type_hints.len()
            };
            for (index, hint) in signature
                .param_type_hints
                .iter()
                .take(fixed_arity)
                .enumerate()
            {
                if matches!(hint, ParamTypeHint::None) {
                    continue;
                }
                let cv_index = signature.param_cv_index(index as u32);
                let slot = (*frame).cv_mut(cv_index) as *mut Value;
                let source = (*slot).dereferenced().clone();
                if source.is_undef() {
                    continue;
                }
                match prepare_call_argument(
                    &source,
                    hint,
                    eg,
                    false,
                    callee_class.as_deref(),
                )? {
                    CallArgumentPreparation::Exact => {}
                    CallArgumentPreparation::Coerced(prepared, _) => {
                        if signature.is_param_by_ref(index as u32) && (*slot).is_reference() {
                            slot_set((*slot).as_ref_ptr(), prepared);
                        } else {
                            frame_slot_set(frame, slot, prepared);
                        }
                    }
                    CallArgumentPreparation::Invalid => {
                        let error = eg.exception.take().unwrap_or_else(|| {
                            let parameter = signature
                                .param_names
                                .get(index)
                                .map(|name| &**name)
                                .unwrap_or("unknown");
                            make_error_value(
                                "TypeError",
                                &format!(
                                    "{}(): Argument #{} (${parameter}) must be of type {}, {} given",
                                    displayed_function_name(eg, func_ptr),
                                    index + 1,
                                    scoped_hint_diagnostic_name(
                                        eg,
                                        frame,
                                        hint,
                                        callee_class.as_deref(),
                                    ),
                                    declared_type_error_value_name(&source),
                                ),
                            )
                        });
                        eg.exception = Some(error.clone());
                        generated_argument_type_error = Some(error);
                        break;
                    }
                }
            }

            if generated_argument_type_error.is_none()
                && signature.is_variadic
                && public_num_args > fixed_arity
                && let Some(hint) = signature.param_type_hints.get(fixed_arity)
                && !matches!(hint, ParamTypeHint::None)
            {
                let variadic_cv = signature.variadic_cv_index;
                let values = (*frame)
                    .cv(variadic_cv)
                    .as_array()
                    .map(|array| {
                        (0..array.len())
                            .filter_map(|position| {
                                array
                                    .get_value_at(position)
                                    .map(|value| value.dereferenced().clone())
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let by_reference = signature.is_param_by_ref(fixed_arity as u32);
                for (position, source) in values.into_iter().enumerate() {
                    match prepare_call_argument(
                        &source,
                        hint,
                        eg,
                        false,
                        callee_class.as_deref(),
                    )? {
                        CallArgumentPreparation::Exact => {}
                        CallArgumentPreparation::Coerced(prepared, _) => {
                            let variadic = (*frame).cv_mut(variadic_cv);
                            if let Some(array) = variadic.as_array_mut() {
                                if by_reference
                                    && array
                                        .get_value_at(position)
                                        .is_some_and(Value::is_reference)
                                {
                                    let _ = array.assign_dereferenced_at(position, prepared);
                                } else {
                                    let _ = array.set_value_at(position, prepared);
                                }
                            }
                        }
                        CallArgumentPreparation::Invalid => {
                            let argument_index = fixed_arity + position;
                            let error = eg.exception.take().unwrap_or_else(|| {
                                make_error_value(
                                    "TypeError",
                                    &format!(
                                        "{}(): Argument #{} must be of type {}, {} given",
                                        displayed_function_name(eg, func_ptr),
                                        argument_index + 1,
                                        scoped_hint_diagnostic_name(
                                            eg,
                                            frame,
                                            hint,
                                            callee_class.as_deref(),
                                        ),
                                        declared_type_error_value_name(&source),
                                    ),
                                )
                            });
                            eg.exception = Some(error.clone());
                            generated_argument_type_error = Some(error);
                            break;
                        }
                    }
                }
            }
        }

        let pending_argument_error = supplied_preentry_error
            .or(generated_preentry_error.as_ref())
            .or(generated_argument_type_error.as_ref());
        if let Some(throwable) = pending_argument_error {
            let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
                .as_deref()
                .is_some_and(crate::stdlib::ini_boolean);
            let trace_options = if ignore_arguments { 2 } else { 0 };
            let saved_trace_num_args = (*frame).num_args;
            if rejects_internal_named_variadic {
                (*frame).num_args = positional_public_num_args as u32;
            }
            let trace = crate::stdlib::collect_debug_backtrace(
                frame,
                trace_options,
                0,
                eg,
                true,
            );
            (*frame).num_args = saved_trace_num_args;
            let function = Function::from_common_ptr(func_ptr);
            let location = if function.fn_type() == FunctionType::User {
                let op_array = &function.as_user().op_array;
                op_array
                    .declaration_line()
                    .filter(|_| !op_array.source_file.is_empty())
                    .map(|line| (op_array.source_file.to_string(), line))
            } else {
                trace_origin.as_ref().map(|(file, line, _, _)| {
                    if file == "Unknown" && *line == 0 {
                        ("[no active file]".to_string(), 0)
                    } else {
                        (file.clone(), *line)
                    }
                })
            };
            if let Some((file, line)) = location
                && let Some(mut object) = throwable.as_object_mut()
            {
                object.set_property("file", Value::string(file));
                object.set_property("line", Value::long(line as i64));
                let trace_key =
                    crate::runtime::throwable_private_property_key(eg, &object, "trace");
                object.set_property(&trace_key, Value::array(trace));
            }
            eg.discard_detached_trace_caller(frame as usize);
            cleanup_frame_slots(frame);
            pop_vm_call_frame(eg, frame);
            return Ok((Value::null(), None));
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
            if let Some(scope) = closure_scope_class_id {
                *generator.tmp_values.last_mut().expect("anonymous scope TMP") =
                    Value::long(i64::from(scope));
            }
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
        FunctionType::Internal => unsafe {
            // SAFETY: `function_type` was read from this live registered
            // descriptor above, so the InternalFunction tail is valid. The
            // VM-stack frame remains live for the synchronous handler; its
            // prior caller link is restored before the detached cleanup path.
            let internal = &*(func_ptr as *const super::function::InternalFunction);
            std::ptr::drop_in_place(&mut return_value as *mut Value);
            // A source opcode that invokes an internal handler through the
            // detached boundary still owns the handler's diagnostic origin.
            // Link that caller only for the synchronous native handler: the
            // frame remains detached for user-code execution and cleanup.
            let previous_caller = (*frame).prev_execute_data;
            if trace_caller_at_current_site
                && !publish_live_trace_caller
                && !trace_caller.is_null()
            {
                (*frame).prev_execute_data = trace_caller;
            }
            let result = (internal.handler)(frame, &mut return_value, eg);
            (*frame).prev_execute_data = previous_caller;
            result
        },
        FunctionType::Undef => {
            eg.exception = Some(make_error_value("Error", "Call to undefined function"));
            Ok(())
        }
    };

    // Detached internal calls bypass DoFcall, but their catchable failures
    // still expose the internal function as frame zero just like an ordinary
    // source call. Snapshot that frame before the detached activation is
    // released; the caller's shared throw boundary supplies the source origin.
    if function_type == FunctionType::Internal
        && let Some(exception) = eg.exception.as_ref()
        && !trace_caller.is_null()
    {
        // SAFETY: both detached frames stay live through this snapshot. Link
        // them only while the shared synchronous trace helper walks the chain.
        unsafe {
            let previous = (*frame).prev_execute_data;
            (*frame).prev_execute_data = trace_caller;
            attach_internal_call_trace_if_missing(exception, frame, trace_caller, eg);
            (*frame).prev_execute_data = previous;
        }
    }

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

    // A detached user callback has no physical predecessor, so its ordinary
    // throw search stops at this frame. Retire its final local owners before
    // popping it just as a linked user frame would; a throwing destructor
    // replaces and chains the callback's pending exception.
    if callback_threw
        && function_type == FunctionType::User
        && let Some(pending) = eg.exception.take()
    {
        // At request shutdown there is no active source frame. PHP lets a
        // local destructor replace the callback's exception without retaining
        // that displaced exception as `previous`; ordinary active-frame
        // unwinds preserve the chain.
        let effective = run_exception_unwind_destructors(
            eg,
            frame,
            pending,
            !saved_execute_data.is_null(),
        )?;
        eg.exception = Some(effective);
    }

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
                    let trace_key =
                        crate::runtime::throwable_private_property_key(eg, &object, "trace");
                    object
                        .get_property("file")
                        .and_then(Value::as_str)
                        .is_some_and(|file| !file.is_empty())
                        && object
                            .get_property("line")
                            .and_then(Value::as_long)
                            .is_some_and(|line| line > 0)
                        && object
                            .get_property(&trace_key)
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
                let trace_key = crate::runtime::throwable_private_property_key(
                    eg, &exception, "trace",
                );
                exception.set_property(&trace_key, Value::array(trace));
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
        None,
        0,
        None,
        None,
        logical_caller,
        true,
        false,
        Some((
            call_file.to_string(),
            call_line,
            Some(throwable),
            false,
        )),
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
        None,
        0,
        None,
        None,
        std::ptr::null_mut(),
        false,
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
    FiberResume(crate::vm::generator::GeneratorFiberInput),
}

#[cold]
fn release_yield_from_values(
    eg: &mut ExecutorGlobals,
    values: Vec<Value>,
    input: GeneratorFrameInput,
    logical_caller: *mut ExecuteData,
) -> Result<GeneratorFrameInput, VmError> {
    let displaced = match &input {
        GeneratorFrameInput::Throw(exception)
        | GeneratorFrameInput::SyntheticThrow(exception)
        | GeneratorFrameInput::Propagate(exception) => exception.clone(),
        GeneratorFrameInput::Send(_)
        | GeneratorFrameInput::YieldFromReturn(_)
        | GeneratorFrameInput::FiberResume(_) => return Ok(input),
    };
    let saved_exception = eg.exception.take();
    let release_result = run_value_destructors_inner(
        eg,
        &values,
        logical_caller,
        false,
        false,
        true,
        false,
    )
    .map(|_| ());
    let replacement = eg.exception.take();
    eg.exception = saved_exception;
    release_result?;
    drop(values);

    let Some(replacement) = replacement else {
        return Ok(input);
    };
    append_replaced_exception(&replacement, &displaced, eg);
    Ok(match input {
        GeneratorFrameInput::Throw(_) => GeneratorFrameInput::Throw(replacement),
        GeneratorFrameInput::SyntheticThrow(_) => {
            GeneratorFrameInput::SyntheticThrow(replacement)
        }
        GeneratorFrameInput::Propagate(_) => GeneratorFrameInput::Propagate(replacement),
        GeneratorFrameInput::Send(_)
        | GeneratorFrameInput::YieldFromReturn(_)
        | GeneratorFrameInput::FiberResume(_) => unreachable!("checked exception input"),
    })
}

#[cold]
fn take_yield_from_temporary_source(
    gen_ref: &crate::vm::generator::GeneratorRef,
) -> Option<Value> {
    let mut generator = gen_ref.borrow_mut();
    let index = std::mem::replace(&mut generator.yield_from_source_tmp, u32::MAX);
    if index == u32::MAX {
        return None;
    }
    let source = generator.tmp_values.get_mut(index as usize)?;
    Some(std::mem::replace(source, Value::undef()))
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

/// Resume the generator method that was interrupted when its active leaf
/// called `Fiber::suspend()`. The public root deliberately remains Running
/// while the Fiber is suspended, so only the Fiber-owned pending input may
/// cross this boundary; unrelated user calls retain the re-entrancy error.
pub(crate) fn resume_generator_from_fiber(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    input: crate::vm::generator::GeneratorFiberInput,
) -> Result<GeneratorResumeOutcome, VmError> {
    use crate::vm::generator::{GeneratorState, YieldFromDelegate};

    let mut probe = Some(gen_ref.clone());
    let mut seen_probe = std::collections::HashSet::new();
    let mut has_suspended_leaf = false;
    while let Some(generator) = probe {
        if !seen_probe.insert(generator_identity(&generator)) {
            break;
        }
        let generator = generator.borrow();
        if generator.fiber_suspended {
            has_suspended_leaf = true;
            break;
        }
        probe = match generator.delegate.as_ref() {
            Some(YieldFromDelegate::Generator(delegate, _)) => Some(delegate.clone()),
            Some(YieldFromDelegate::Array(_, _, _))
            | Some(YieldFromDelegate::Iterator(_))
            | None => None,
        };
    }
    if !has_suspended_leaf {
        return Err(VmError::Fatal(
            "Fiber-owned generator continuation has no suspended activation".into(),
        ));
    }
    if matches!(input, crate::vm::generator::GeneratorFiberInput::ForceClose(_)) {
        let mut current = Some(gen_ref.clone());
        let mut seen = std::collections::HashSet::new();
        while let Some(generator) = current {
            if !seen.insert(generator_identity(&generator)) {
                break;
            }
            generator.borrow_mut().force_closing = true;
            current = match generator.borrow().delegate.as_ref() {
                Some(YieldFromDelegate::Generator(delegate, _)) => Some(delegate.clone()),
                Some(YieldFromDelegate::Array(_, _, _))
                | Some(YieldFromDelegate::Iterator(_))
                | None => None,
            };
        }
    }
    resume_generator_delegation(
        eg,
        gen_ref,
        GeneratorFrameInput::FiberResume(input),
        false,
    )
}

/// Retire a suspended generator whose last userland owner is being released.
/// PHP skips the abandoned body, executes every enclosing finally block and
/// rejects any attempt to suspend again from that force-close path.
#[cold]
pub(crate) fn force_close_generator(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    logical_caller_at_current_site: bool,
) -> Result<(), VmError> {
    use crate::vm::generator::YieldFromDelegate;

    // A suspended `yield from` owns a nested activation whose finally blocks
    // and locals must retire before the delegating frame. Walk this sparse
    // chain iteratively so valid deep delegation cannot overflow Rust's stack.
    let mut chain = Vec::new();
    let mut current = Some(gen_ref.clone());
    let mut seen = std::collections::HashSet::new();
    while let Some(generator) = current {
        let identity = std::rc::Rc::as_ptr(&generator) as usize;
        if !seen.insert(identity) {
            break;
        }
        current = match generator.borrow().delegate.as_ref() {
            Some(YieldFromDelegate::Generator(delegate, _))
                if delegate
                    .borrow()
                    .owner_object
                    .as_ref()
                    .is_none_or(|owner| owner.strong_count() <= 1) =>
            {
                Some(delegate.clone())
            }
            Some(YieldFromDelegate::Generator(_, _)) => None,
            Some(YieldFromDelegate::Array(_, _, _))
            | Some(YieldFromDelegate::Iterator(_))
            | None => None,
        };
        chain.push(generator);
    }
    while let Some(generator) = chain.pop() {
        let close_result = force_close_generator_activation(
            eg,
            &generator,
            logical_caller_at_current_site,
        );
        // Force-close runs only after the last public Generator owner has
        // disappeared. The internal GeneratorRef can outlive that object
        // briefly while release bookkeeping unwinds; do not let that private
        // handle extend the observable lifetime of the creating Closure.
        // Normal and exceptional completion retain the Closure until the
        // still-visible Generator object itself is released.
        {
            let mut generator = generator.borrow_mut();
            generator.closure_owner = None;
            generator.extra_args.clear();
        }
        close_result?;
    }
    Ok(())
}

#[cold]
fn force_close_generator_activation(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    logical_caller_at_current_site: bool,
) -> Result<(), VmError> {
    use crate::vm::generator::GeneratorState;

    let state = gen_ref.borrow().state;
    if matches!(state, GeneratorState::Completed | GeneratorState::Running) {
        return Ok(());
    }

    let (func, ip_offset) = {
        let generator = gen_ref.borrow();
        (generator.func, generator.ip_offset)
    };
    // SAFETY: generator construction retains a stable request-owned user
    // function pointer until the Generator payload is dropped.
    let user = unsafe { &*(func as *const UserFunction) };
    let finally_start = (state == GeneratorState::Suspended)
        .then(|| {
            user.op_array
                .try_entries
                .iter()
                .filter(|entry| {
                    entry.finally_start != u32::MAX
                        && ip_offset >= entry.try_start as usize
                        && ip_offset < entry.finally_start as usize
                })
                .min_by_key(|entry| entry.finally_end - entry.try_start)
                .map(|entry| entry.finally_start as usize)
        })
        .flatten();

    let (frame, saved_execute_data) = materialize_generator_frame(eg, gen_ref);
    if logical_caller_at_current_site {
        eg.publish_detached_trace_caller_at_current_site(
            frame as usize,
            saved_execute_data as usize,
        );
    }
    eg.current_execute_data.set(frame);
    let release_result = release_force_closed_generator_temps(
        eg,
        frame,
        &user.op_array,
        ip_offset,
        finally_start,
    );
    eg.current_execute_data.set(saved_execute_data);
    release_result?;
    {
        let mut generator = gen_ref.borrow_mut();
        generator.force_closing = true;
        generator.delegate = None;
    }
    let Some(finally_start) = finally_start else {
        close_failed_generator(gen_ref);
        eg.current_execute_data.set(frame);
        run_frame_destructors(eg, frame)?;
        eg.current_execute_data.set(saved_execute_data);
        unsafe { cleanup_frame_slots(frame) };
        pop_vm_call_frame(eg, frame);
        return Ok(());
    };

    // A force-close is represented as a value-less non-local return. The
    // ordinary finally completion machinery already walks nested outer
    // finally ranges and retires the detached frame at the last marker.
    unsafe {
        (*frame).pending_return_after_finally = true;
        (*frame).opline = user.op_array.instructions.as_ptr().add(finally_start);
    }
    gen_ref.borrow_mut().pending_return_after_finally = true;
    match execute_resumed_generator_frame(
        eg,
        gen_ref,
        frame,
        saved_execute_data,
        None,
        false,
        false,
        false,
    )? {
        GeneratorResumeOutcome::Advanced => Ok(()),
        GeneratorResumeOutcome::Threw(exception) => {
            eg.exception = Some(exception);
            Ok(())
        }
    }
}

#[cold]
fn release_force_closed_generator_temps(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    ip_offset: usize,
    finally_start: Option<usize>,
) -> Result<(), VmError> {
    // A force-close is an abrupt exit from every active foreach. Generator
    // returns intentionally do not carry ordinary return-cleanup markers, so
    // recover the compiler-owned iteration source from the enclosing loop
    // shape and retire it before entering finally. Nested loops are ordered
    // innermost first, matching PHP's unwind order.
    for index in (0..ip_offset.min(op_array.instructions.len())).rev() {
        let next = &op_array.instructions[index];
        if !matches!(
            next.opcode,
            OpCode::ForeachNext | OpCode::ForeachNextRef | OpCode::ForeachNextPlain
        ) {
            continue;
        }
        let Some(exit) = op_array.instructions.get(index + 1) else {
            continue;
        };
        if exit.opcode != OpCode::JmpZ || usize::from(exit.op2) <= ip_offset {
            continue;
        }
        release_statement_temps(
            eg,
            frame,
            next.op1 as usize,
            next.op1 as usize + 1,
            STATEMENT_TEMPS_NESTED_OBJECTS,
            false,
        )?;
    }

    let Some(finally_start) = finally_start else {
        return Ok(());
    };
    let end = finally_start.min(op_array.instructions.len());
    if ip_offset >= end {
        return Ok(());
    }
    let window = &op_array.instructions[ip_offset..end];
    let Some(first_release) = window.iter().find(|instruction| {
        instruction.opcode == OpCode::ReleaseTemps
            && instruction._pad & (RELEASE_TEMPS_ON_RETURN | RELEASE_TEMPS_SUBEXPRESSION) == 0
    }) else {
        return Ok(());
    };
    let release = if first_release._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0 {
        first_release
    } else {
        window
            .iter()
            .find(|candidate| {
                candidate.opcode == OpCode::ReleaseTemps
                    && candidate._pad & RELEASE_TEMPS_ON_RETURN == 0
                    && candidate._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0
                    && candidate.op1 <= first_release.op1
                    && candidate.op2 >= first_release.op2
            })
            .unwrap_or(first_release)
    };
    release_statement_temps(
        eg,
        frame,
        release.op1 as usize,
        release.op2 as usize,
        if release._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0 {
            STATEMENT_TEMPS_NESTED_OBJECTS
        } else {
            STATEMENT_TEMPS_ORDINARY
        },
        false,
    )
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
    restore_generator_resume_value(frame, gen_ref, None, send_value);
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
    // Once a Fiber owns this continuation, every Running parent in the
    // yield-from chain belongs to the same resume operation. Completion or an
    // exception replaces the leaf input while propagating upward, but must
    // not accidentally re-enable the public re-entrancy rejection mid-chain.
    let fiber_owned_resume = matches!(&input, GeneratorFrameInput::FiberResume(_));

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
                            parent_data.last_yielded_value =
                                parent_data.value.clone_closure_capture();
                            parent_data.last_yielded_key =
                                parent_data.key.clone_closure_capture();
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
                            Some(YieldFromDelegate::Array(_, _, _))
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
                            &parents,
                            None,
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
            GeneratorState::Running if fiber_owned_resume => {}
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
                Some(YieldFromDelegate::Array(_, _, _))
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
            if mode == crate::vm::generator::YieldFromGeneratorMode::Traversable
                && !fiber_owned_resume
            {
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
                    match execute_generator_frame_input(
                        eg,
                        &current,
                        frame_input,
                        &parents,
                        None,
                    )? {
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
            Some(YieldFromDelegate::Array(_, _, _))
        );
        if array_delegate {
            let delegate = current
                .borrow_mut()
                .delegate
                .take()
                .expect("array delegation disappeared");
            let YieldFromDelegate::Array(entries, position, external_byte_keys) = delegate else {
                unreachable!();
            };
            match std::mem::replace(
                &mut input,
                GeneratorFrameInput::Send(Value::null()),
            ) {
                frame_input @ (GeneratorFrameInput::Throw(_)
                | GeneratorFrameInput::SyntheticThrow(_)
                | GeneratorFrameInput::Propagate(_)) => {
                    let mut values = entries
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect::<Vec<_>>();
                    values.extend(take_yield_from_temporary_source(&current));
                    match execute_generator_frame_input(
                        eg,
                        &current,
                        frame_input,
                        &parents,
                        Some(values),
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
                GeneratorFrameInput::Send(_)
                | GeneratorFrameInput::YieldFromReturn(_)
                | GeneratorFrameInput::FiberResume(_) => {
                    if position >= entries.len() {
                        current.borrow_mut().ip_offset += 1;
                        match execute_generator_frame_input(
                            eg,
                            &current,
                            GeneratorFrameInput::YieldFromReturn(Value::null()),
                            &parents,
                            None,
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
                                crate::value::ArrayKey::String(key) if external_byte_keys => {
                                    Value::binary_string_from_storage(key.clone())
                                }
                                crate::value::ArrayKey::String(key) => Value::string(key),
                            };
                            (value.clone(), key)
                        };
                        {
                            let mut current_data = current.borrow_mut();
                            current_data.value = value;
                            current_data.key = key;
                            current_data.last_yielded_value =
                                current_data.value.clone_closure_capture();
                            current_data.last_yielded_key =
                                current_data.key.clone_closure_capture();
                            current_data.delegate = Some(YieldFromDelegate::Array(
                                entries,
                                position + 1,
                                external_byte_keys,
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
                    let mut values = vec![iterator];
                    values.extend(take_yield_from_temporary_source(&current));
                    match execute_generator_frame_input(
                        eg,
                        &current,
                        frame_input,
                        &parents,
                        Some(values),
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
                GeneratorFrameInput::Send(_)
                | GeneratorFrameInput::YieldFromReturn(_)
                | GeneratorFrameInput::FiberResume(_) => {
                    let step = yield_from_iterator_step(eg, &iterator, false)?;
                    if let Some(exception) = eg.exception.take() {
                        match execute_generator_frame_input(
                            eg,
                            &current,
                            GeneratorFrameInput::Propagate(exception),
                            &parents,
                            None,
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
                            current_data.last_yielded_value =
                                current_data.value.clone_closure_capture();
                            current_data.last_yielded_key =
                                current_data.key.clone_closure_capture();
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
                            &parents,
                            None,
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
        match execute_generator_frame_input(eg, &current, frame_input, &parents, None)? {
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
    mut input: GeneratorFrameInput,
    trace_parents: &[crate::vm::generator::GeneratorRef],
    release_values: Option<Vec<Value>>,
) -> Result<GeneratorFrameOutcome, VmError> {
    let saved_execute_data = eg.current_execute_data.get();
    let trace_frames = materialize_generator_trace_frames(eg, trace_parents, saved_execute_data);
    let (frame, saved_execute_data) = materialize_generator_frame(eg, gen_ref);
    if let Some(parent) = trace_frames.last() {
        eg.publish_detached_trace_caller_at_current_site(frame as usize, *parent as usize);
    }
    if let Some(values) = release_values {
        input = release_yield_from_values(eg, values, input, frame)?;
    }
    let escaped_same_input_extends = match &input {
        GeneratorFrameInput::Throw(exception) => Some((exception.object_identity(), false)),
        GeneratorFrameInput::SyntheticThrow(exception)
        | GeneratorFrameInput::Propagate(exception) => {
            Some((exception.object_identity(), true))
        }
        GeneratorFrameInput::Send(_)
        | GeneratorFrameInput::YieldFromReturn(_)
        | GeneratorFrameInput::FiberResume(_) => None,
    };
    let (injected_exception, seed_injected_trace, extend_injected_trace) = match input {
        GeneratorFrameInput::Send(value) => {
            restore_generator_resume_value(frame, gen_ref, None, value);
            (None, false, false)
        }
        GeneratorFrameInput::Throw(exception) => (Some(exception), false, false),
        GeneratorFrameInput::SyntheticThrow(exception) => (Some(exception), true, false),
        GeneratorFrameInput::Propagate(exception) => (Some(exception), false, true),
        GeneratorFrameInput::YieldFromReturn(value) => {
            restore_yield_from_result(frame, gen_ref, value);
            (None, false, false)
        }
        GeneratorFrameInput::FiberResume(input) => {
            let result_slot = {
                let mut generator = gen_ref.borrow_mut();
                generator.fiber_suspended = false;
                generator.fiber_suspend_result_slot.take()
            };
            match input {
                crate::vm::generator::GeneratorFiberInput::Resume(value) => {
                    if let Some(result_slot) = result_slot {
                        restore_generator_resume_value(
                            frame,
                            gen_ref,
                            Some(result_slot),
                            value,
                        );
                    }
                    (None, false, false)
                }
                crate::vm::generator::GeneratorFiberInput::Throw(exception)
                | crate::vm::generator::GeneratorFiberInput::ForceClose(exception) => {
                    (Some(exception), false, false)
                }
            }
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
    );
    release_generator_trace_frames(eg, trace_frames);
    let outcome = outcome?;
    if matches!(outcome, GeneratorResumeOutcome::Advanced)
        && !trace_parents.is_empty()
        && gen_ref.borrow().state == crate::vm::generator::GeneratorState::Suspended
        && matches!(
            gen_ref.borrow().delegate.as_ref(),
            Some(crate::vm::generator::YieldFromDelegate::Generator(_, _))
        )
    {
        gen_ref.borrow_mut().indirectly_primed = true;
    }
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

#[cold]
fn materialize_generator_trace_frames(
    eg: &mut ExecutorGlobals,
    parents: &[crate::vm::generator::GeneratorRef],
    saved_execute_data: *mut ExecuteData,
) -> Vec<*mut ExecuteData> {
    // Debug traces need the logical yield-from chain, but valid user programs
    // can delegate hundreds of thousands of generators without requesting a
    // trace. Bound the temporary diagnostic reconstruction so ordinary deep
    // traversal remains iterative and cannot exhaust the VM stack.
    if parents.len() > 64 {
        return Vec::new();
    }
    let mut frames = Vec::with_capacity(parents.len());
    let mut caller = saved_execute_data;
    for (index, parent) in parents.iter().enumerate() {
        let parent = parent.borrow();
        let public_num_args = parent
            .trace_num_args
            .as_long()
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or(0);
        let frame = eg.vm_stack.push_call_frame(
            parent.func,
            0,
            public_num_args,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if index == 0 {
            eg.publish_detached_trace_caller(frame as usize, caller as usize);
        } else {
            eg.publish_detached_trace_caller_at_current_site(frame as usize, caller as usize);
        }
        eg.publish_debug_only_trace_frame(frame as usize);
        if !parent.extra_args.is_empty() {
            let first = unsafe { (*parent.func).sig.public_arity() };
            eg.publish_function_arguments(
                frame as usize,
                crate::runtime::FunctionArgumentSnapshot {
                    first,
                    values: parent.extra_args.clone(),
                },
            );
        }
        // SAFETY: these trace-only frames use the same immutable function and
        // compiler-sized CV/TMP layout as their suspended generator snapshots.
        unsafe {
            let user = &*(parent.func as *const UserFunction);
            (*frame).opline = user
                .op_array
                .instructions
                .as_ptr()
                .add(parent.ip_offset);
            for (slot, value) in parent.cv_values.iter().enumerate() {
                frame_restore_slot(frame, (*frame).cv_mut(slot as u32), value.clone_closure_capture());
            }
            for (slot, value) in parent.tmp_values.iter().enumerate() {
                frame_restore_slot(frame, (*frame).tmp_mut(slot as u32), value.clone_closure_capture());
            }
            if user.op_array.is_anonymous()
                && parent
                    .tmp_values
                    .last()
                    .is_some_and(|value| value.as_long().is_some())
            {
                (*frame).set_closure_scope();
            }
        }
        caller = frame;
        frames.push(frame);
    }
    frames
}

#[cold]
fn release_generator_trace_frames(
    eg: &mut ExecutorGlobals,
    frames: Vec<*mut ExecuteData>,
) {
    for frame in frames.into_iter().rev() {
        eg.discard_detached_trace_caller(frame as usize);
        // SAFETY: trace frames were allocated in this order on the VM stack,
        // never executed, and are retired in strict reverse order.
        unsafe { cleanup_frame_slots(frame) };
        pop_vm_call_frame(eg, frame);
    }
}

/// Materialize one detached frame from the generator snapshot. All resume
/// paths use this function so slot restoration and frame ownership cannot
/// drift between normal yield, delegated return and delegated exception.
fn materialize_generator_frame(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
) -> (*mut ExecuteData, *mut ExecuteData) {
    let (
        func_ptr,
        public_num_args,
        called_scope_class_id,
        closure_static_vars,
        pending_return_after_finally,
        ip_offset,
        closure_scope,
        cv_values,
        tmp_values,
        extra_args,
        pending_finally_exceptions,
    ) = {
        let mut generator = gen_ref.borrow_mut();
        generator.state = crate::vm::generator::GeneratorState::Running;
        let closure_scope = generator
            .tmp_values
            .last()
            .is_some_and(|value| value.as_long().is_some());
        (
            generator.func,
            generator
                .trace_num_args
                .as_long()
                .and_then(|count| u32::try_from(count).ok())
                .unwrap_or(0),
            generator.called_scope_class_id,
            generator.closure_static_vars.clone(),
            generator.pending_return_after_finally,
            generator.ip_offset,
            closure_scope,
            std::mem::take(&mut generator.cv_values),
            std::mem::take(&mut generator.tmp_values),
            generator.extra_args.clone(),
            std::mem::take(&mut generator.pending_finally_exceptions),
        )
    };
    let saved_execute_data = eg.current_execute_data.get();
    let frame = eg.vm_stack.push_call_frame(
        func_ptr,
        0,
        public_num_args,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    eg.publish_detached_trace_caller(frame as usize, saved_execute_data as usize);
    publish_late_static_call_class_id(eg, frame, called_scope_class_id);
    if let Some(storage) = closure_static_vars {
        eg.publish_closure_static_vars(frame as usize, storage);
    }
    if !extra_args.is_empty() {
        let first = unsafe { (*func_ptr).sig.public_arity() };
        eg.publish_function_arguments(
            frame as usize,
            crate::runtime::FunctionArgumentSnapshot {
                first,
                values: extra_args,
            },
        );
    }
    if !pending_finally_exceptions.is_empty() {
        eg.finally_exceptions
            .insert(frame as usize, pending_finally_exceptions);
    }
    // SAFETY: push_call_frame returned this live compiler-sized generator
    // frame; every restored CV/TMP index comes from its retained snapshot and
    // ip_offset belongs to the same immutable generator op-array.
    unsafe {
        let user = &*(func_ptr as *const UserFunction);
        (*frame).return_value = std::ptr::null_mut();
        (*frame).pending_return_after_finally = pending_return_after_finally;
        for (i, value) in cv_values.into_iter().enumerate() {
            let slot = (*frame).cv_mut(i as u32);
            frame_restore_slot(frame, slot as *mut Value, value);
        }
        for (i, value) in tmp_values.into_iter().enumerate() {
            let slot = (*frame).tmp_mut(i as u32);
            frame_restore_slot(frame, slot as *mut Value, value);
        }
        if user.op_array.is_anonymous() && closure_scope {
            (*frame).set_closure_scope();
        }
        (*frame).opline = user
            .op_array
            .instructions
            .as_ptr()
            .add(ip_offset)
    };
    (frame, saved_execute_data)
}

#[inline(always)]
fn restore_generator_resume_value(
    frame: *mut ExecuteData,
    gen_ref: &crate::vm::generator::GeneratorRef,
    fiber_result_slot: Option<u32>,
    send_value: Value,
) {
    let gen_data = gen_ref.borrow();
    if fiber_result_slot.is_none() && gen_data.ip_offset == 0 {
        return;
    }
    // SAFETY: materialize_generator_frame created this live frame from the same
    // retained function and snapshot. The Fiber result slot was recorded from
    // that exact activation; otherwise the immutable preceding Yield provides
    // a compiler-sized result slot in the same TMP envelope.
    unsafe {
        if let Some(result_slot) = fiber_result_slot {
            frame_slot_set(frame, (*frame).slot_mut(result_slot), send_value);
            return;
        }
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
fn complete_escaped_generator_origin_trace(
    exception: &Value,
    eg: &ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    frame: *mut ExecuteData,
    saved_execute_data: *mut ExecuteData,
) {
    if saved_execute_data.is_null() {
        return;
    }
    let continuation =
        generator_resume_continuation_trace(eg, gen_ref, frame, saved_execute_data);
    let Some(origin_frame) = continuation.values().next().cloned() else {
        return;
    };
    let Some(object) = exception.as_object() else {
        return;
    };
    let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
    let existing = object
        .get_property(&trace_key)
        .and_then(Value::as_array)
        .map(|trace| trace.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    let same_frame = |candidate: &Value| {
        let Some(candidate) = candidate.as_array() else {
            return false;
        };
        let Some(origin) = origin_frame.as_array() else {
            return false;
        };
        ["function", "class", "type"].into_iter().all(|key| {
            let left = candidate.get_str(key).and_then(Value::as_str);
            let right = origin.get_str(key).and_then(Value::as_str);
            left == right
        })
    };
    if existing.iter().any(same_frame) {
        return;
    }

    if existing.is_empty() {
        drop(object);
        if let Some(mut object) = exception.as_object_mut() {
            object.set_property(&trace_key, Value::array(continuation));
        }
        return;
    }

    let mut complete = PhpArray::new();
    complete.push(origin_frame);
    for entry in existing {
        complete.push(entry);
    }
    drop(object);
    if let Some(mut object) = exception.as_object_mut() {
        object.set_property(&trace_key, Value::array(complete));
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
            let trace_key =
                crate::runtime::throwable_private_property_key(eg, &object, "trace");
            object
                .get_property(&trace_key)
                .and_then(Value::as_array)
                .map(|trace| trace.values().cloned().collect::<Vec<_>>())
        })
        .unwrap_or_default();
    if saved_execute_data.is_null()
        || !((seed_trace && existing_trace.is_empty()) || extend_trace)
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
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        object.set_property(&trace_key, Value::array(trace));
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
        match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, _) => execute_ex(eg, new_frame),
            ThrowResult::Unhandled(exception) => {
                eg.exception = Some(exception);
                Ok(())
            }
        }
    } else {
        execute_ex(eg, frame)
    };
    let fiber_suspended = matches!(&result, Err(VmError::Fatal(message)) if message.is_empty())
        && gen_ref.borrow().fiber_suspended;
    let escaped_exception = eg.exception.take();
    if let Some(exception) = escaped_exception.as_ref()
        && exception.object_identity() != injected_exception_identity
    {
        // The detached activation is intentionally omitted from ordinary
        // live backtrace traversal to avoid duplicating yield-from parents.
        // If an exception is created inside that activation, restore exactly
        // its missing origin frame before the materialized frame is retired;
        // each delegating boundary is still appended separately on unwind.
        complete_escaped_generator_origin_trace(
            exception,
            eg,
            gen_ref,
            frame,
            saved_execute_data,
        );
    }

    if fiber_suspended {
        let (cv_values, tmp_values) = {
            let mut generator = gen_ref.borrow_mut();
            (
                std::mem::take(&mut generator.cv_values),
                std::mem::take(&mut generator.tmp_values),
            )
        };
        let snapshot = snapshot_suspended_generator_frame(
            frame,
            None,
            false,
            cv_values,
            tmp_values,
        );
        let pending_finally_exceptions = eg
            .finally_exceptions
            .remove(&(frame as usize))
            .unwrap_or_default();
        let mut generator = gen_ref.borrow_mut();
        generator.cv_values = snapshot.cv_values;
        generator.tmp_values = snapshot.tmp_values;
        generator.ip_offset = snapshot.ip_offset;
        generator.pending_return_after_finally = snapshot.pending_return_after_finally;
        generator.pending_finally_exceptions = pending_finally_exceptions;
        // Running remains observable while the owning Fiber is suspended.
        generator.state = crate::vm::generator::GeneratorState::Running;
    }

    let closing_after_exception = escaped_exception.is_some() && !fiber_suspended;
    if closing_after_exception {
        // The materialized frame and the suspended Generator snapshot own the
        // same PHP values. Retire the snapshot first so frame cleanup observes
        // the real last owners and can run user destructors while the escaping
        // exception is still available for replacement chaining.
        close_failed_generator(gen_ref);
        eg.exception = escaped_exception.clone();
    }
    cleanup_detached_frame_chain(
        eg,
        frame,
        closing_after_exception,
        closing_after_exception,
    )?;
    let escaped_exception = if closing_after_exception {
        eg.exception.take()
    } else {
        escaped_exception
    };
    eg.current_execute_data.set(saved_execute_data);
    eg.active_generator = saved_active;
    if let Err(error) = result {
        if fiber_suspended {
            eg.exception = saved_exception;
            return Err(error);
        }
        close_failed_generator(gen_ref);
        return Err(error);
    }
    if let Some(exception) = escaped_exception {
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
    generator.yield_from_source_tmp = u32::MAX;
    generator.cv_values.clear();
    generator.tmp_values.clear();
    generator.pending_finally_exceptions.clear();
    generator.fiber_suspended = false;
    generator.fiber_suspend_result_slot = None;
    generator.fiber_resume_input = None;
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
    detached_caller_at_current_site: bool,
) -> Result<(), VmError> {
    let mut pending_exception = eg.exception.take();
    let mut cleanup_error = None;
    let mut frame = eg.current_execute_data.get();
    while !frame.is_null() {
        let previous = unsafe { (*frame).prev_execute_data };
        if run_destructors {
            loop {
                let destructor_result = if detached_caller_at_current_site {
                    run_detached_frame_destructors(eg, frame)
                } else {
                    run_frame_destructors(eg, frame)
                };
                if let Err(error) = destructor_result {
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
        eg.discard_detached_trace_caller(frame as usize);
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

/// Recreate the Generator internal-method frame that was retired while a
/// Fiber suspension unwound the synchronous Rust handler. The caller frame
/// and its DoFcall stay live on the Fiber stack; rebuilding only the small
/// internal frame lets the canonical handler finish its result/exception
/// contract without re-running Init/Send opcodes or retaining stale pointers.
pub(crate) fn resume_suspended_generator_call(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    function: *const FunctionCommon,
    public_num_args: u32,
    arguments: Vec<Value>,
    return_value: *mut Value,
) -> Result<Option<*mut ExecuteData>, VmError> {
    // SAFETY: the suspension owns a stable registered Generator method and a
    // still-live caller frame on the inactive Fiber stack. The captured slots
    // were cloned before the original internal frame was retired.
    unsafe {
        let common = &*function;
        if common.fn_type != FunctionType::Internal {
            return Err(VmError::Fatal(
                "Suspended Generator call is not an internal method".into(),
            ));
        }
        let call = eg.vm_stack.push_call_frame(
            function,
            arguments.len() as u32,
            public_num_args,
            frame,
            std::ptr::null_mut(),
        );
        (*call).return_value = return_value;
        for (index, argument) in arguments.into_iter().enumerate() {
            frame_slot_init(call, (*call).cv_mut(index as u32), argument);
        }

        let result_type = if return_value.is_null() {
            OpType::Unused
        } else {
            (*(*frame).opline).result_type
        };
        if !return_value.is_null() {
            frame_result_prepare_external_write(frame, return_value, result_type);
        }
        eg.current_execute_data.set(frame);
        let internal = &*(function as *const super::function::InternalFunction);
        let handler_result = (internal.handler)(call, return_value, eg);
        eg.current_execute_data.set(frame);
        if !return_value.is_null() {
            frame_result_finish_external_write(frame, return_value, result_type);
        }
        let internal_exception = eg.exception.take();
        if let Some(exception) = internal_exception.as_ref() {
            attach_internal_call_trace_if_missing(exception, call, frame, eg);
        }
        cleanup_frame_slots(call);
        pop_vm_call_frame(eg, call);

        if let Some(exception) = internal_exception {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, _) => Some(new_frame),
                ThrowResult::Unhandled(exception) => {
                    eg.exception = Some(exception);
                    None
                }
            });
        }
        handler_result?;
        (*frame).opline = (*frame).opline.add(1);
        Ok(Some(frame))
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
        initialize_bound_this_frame(
            frame, callback.func_ptr, callback.bound_this.clone(), callback.closure_scope_class_id,
        );
    }
    if callback.called_scope_class_id != 0 {
        publish_late_static_call_class_id(eg, frame, callback.called_scope_class_id);
    }
    if let Some(storage) = callback.closure_static_vars.clone() {
        eg.publish_closure_static_vars(frame as usize, storage);
    }
    eg.publish_function_arguments(
        frame as usize,
        crate::runtime::FunctionArgumentSnapshot {
            first: 0,
            values: arguments.to_vec(),
        },
    );
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
            CallArgumentPreparation::Coerced(prepared, _diagnostic) => {
                let slot = (*frame).cv_mut(cv_index) as *mut Value;
                if (*slot).is_reference() {
                    slot_set((*slot).as_ref_ptr(), prepared);
                } else {
                    frame_slot_set(frame, slot, prepared);
                }
            },
            CallArgumentPreparation::Invalid => {
                if let Some(exception) = eg.exception.take() {
                    argument_error = Some(exception);
                    break;
                }
                let parameter = common
                    .sig
                    .param_names
                    .get(index)
                    .map(|name| &**name)
                    .unwrap_or("unknown");
                argument_error = Some(make_error_value(
                    "TypeError",
                    &format!(
                        "{}(): Argument #{} (${parameter}) must be of type {}, {} given",
                        displayed_function_name(eg, callback.func_ptr),
                        index + 1,
                        scoped_hint_diagnostic_name(eg, frame, hint, callee_class.as_deref()),
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
            && object
                .get_property("file")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            object.set_property(
                "file",
                Value::shared_string(user.op_array.source_file.clone()),
            );
            object.set_property("line", Value::long(line as i64));
            let trace_key =
                crate::runtime::throwable_private_property_key(eg, &object, "trace");
            object.set_property(&trace_key, Value::array(trace));
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
) -> Result<Option<*mut ExecuteData>, VmError> {
    Ok(match throw_in_frame(eg, frame, exception)? {
        ThrowResult::Handled(new_frame, _) => {
            eg.current_execute_data.set(new_frame);
            Some(new_frame)
        }
        ThrowResult::Unhandled(exception) => {
            eg.exception = Some(exception);
            None
        }
    })
}
