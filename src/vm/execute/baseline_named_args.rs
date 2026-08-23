// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[cold]
fn named_argument_reference_error(
    eg: &ExecutorGlobals,
    func_common: &FunctionCommon,
    parameter_index: u32,
) -> Value {
    let parameter_name = func_common
        .sig
        .param_names
        .get(parameter_index as usize)
        .map(String::as_str)
        .unwrap_or("unknown");
    let function_name = registered_function_name(eg, func_common as *const FunctionCommon);
    make_error_value(
        "Error",
        &format!(
            "{}(): Argument #{} (${}) could not be passed by reference",
            function_name,
            parameter_index + 1,
            parameter_name
        ),
    )
}

#[inline(never)]
fn op_send_named<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // Named argument: op1=value, op2=CONST name string
    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let name = name_val.as_str().unwrap_or("");
    let call = unsafe { (*frame).call };
    debug_assert!(!call.is_null());
    let func_common = unsafe { &*(*call).func };
    let yield_snapshot = opline._pad & SEND_FLAG_YIELD_SNAPSHOT != 0;

    if !unsafe { (*call).named_args_used } {
        let positional = opline.extended_value.min(func_common.sig.public_arity());
        prepare_named_call_frame(eg, call, func_common, positional);
    }

    // Find the parameter position by name
    let mut resolved_idx: Option<u32> = None;
    for (idx, pname) in func_common.sig.param_names.iter().enumerate() {
        if pname == name {
            resolved_idx = Some(idx as u32);
            break;
        }
    }

    // Determine if the resolved index targets the variadic parameter itself.
    let public_max = func_common.sig.public_arity();
    let is_variadic_target = func_common.sig.is_variadic && match resolved_idx {
        Some(idx) => idx >= public_max,
        None => true,
    };

    if is_variadic_target {
        let internal_function =
            func_common.fn_type == crate::vm::function::FunctionType::Internal;
        let registered_name = internal_function
            .then(|| registered_function_name(eg, func_common as *const FunctionCommon));
        let forwards_named_arguments = registered_name
            .as_deref()
            .is_some_and(crate::stdlib::internal_variadic_forwards_named_arguments);
        if !func_common.sig.is_variadic
            || (internal_function && !forwards_named_arguments)
        {
            let err = if internal_function && func_common.sig.is_variadic {
                make_error_value(
                    "ArgumentCountError",
                    &format!(
                        "{}() does not accept unknown named parameters",
                        registered_name.as_deref().unwrap_or("unknown")
                    ),
                )
            } else {
                make_error_value("Error", &format!("Unknown named parameter ${}", name))
            };
            // SAFETY: `call` is the non-null pending call owned by this live frame;
            // the error path consumes and retires it exactly once.
            match unsafe { cleanup_call_and_throw(eg, frame, call, err) }? {
                ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
            }
        }

        // Duplicate check: scan the pending buffer for this name
        let call_key = call as usize;
        if let Some(existing) = eg.pending_named_variadic.get(&call_key) {
            if existing.iter().any(|(n, _)| n == name) {
                let err = make_error_value("Error", &format!(
                    "Named parameter ${} overwrites previous argument", name
                ));
                // SAFETY: `call` is the non-null pending call owned by this live frame;
                // the duplicate-argument path consumes and retires it exactly once.
                match unsafe { cleanup_call_and_throw(eg, frame, call, err) }? {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }

        let variadic_index = func_common.sig.param_names.len().saturating_sub(1) as u32;
        let is_ref = func_common.sig.is_param_by_ref(variadic_index);
        let value = if is_ref {
            unsafe {
                if opline._pad & SEND_FLAG_NONREFERENCEABLE != 0
                    && !func_common.sig.is_param_prefer_ref(variadic_index)
                {
                    let error = named_argument_reference_error(eg, func_common, variadic_index);
                    return Ok(match cleanup_call_and_throw(eg, frame, call, error)? {
                        ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                    });
                }
                let reference_source = if yield_snapshot {
                    debug_assert_eq!(opline.result_type, OpType::Unused);
                    let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                    Some(base.add(opline.result as usize))
                } else if opline.op1_type == OpType::Cv {
                    let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                    Some(base.add(opline.op1 as usize))
                } else if matches!(opline.op1_type, OpType::Tmp | OpType::Var) {
                    let source = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                    (*source).is_reference().then_some(source)
                } else {
                    None
                };
                if let Some(source) = reference_source {
                    materialize_reference_alias(frame, source)
                } else if !func_common.sig.is_param_prefer_ref(variadic_index) {
                    let error = named_argument_reference_error(eg, func_common, variadic_index);
                    return Ok(match cleanup_call_and_throw(eg, frame, call, error)? {
                        ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                    });
                } else {
                    snapshot_runtime_send_rvalue(eg, frame, op_array, opline)?
                }
            }
        } else {
            snapshot_runtime_send_rvalue(eg, frame, op_array, opline)?
        };
        if let Some(exception) = eg.exception.take() {
            // SAFETY: `call` is the non-null pending call owned by this live frame;
            // propagating the callback exception consumes it exactly once.
            match unsafe { cleanup_call_and_throw(eg, frame, call, exception) }? {
                ThrowResult::Handled(nf, no) => return Ok(ColdResult::NewFrame(nf, no)),
                ThrowResult::Unhandled(t) => return Ok(ColdResult::Unhandled(t)),
            }
        }
        eg.pending_named_variadic
            .entry(call_key)
            .or_insert_with(Vec::new)
            .push((name.to_string(), value));
        // This named arg doesn't occupy a CV slot, so decrement
        // num_args so DoFcall's positional variadic count is correct.
        unsafe {
            if (*call).num_args > 0 {
                (*call).num_args -= 1;
            }
        }
    } else {
        match resolved_idx {
            Some(idx) => {
                let cv_idx = func_common.sig.param_cv_index(idx);

                // Check for duplicate: if CV slot already has a non-undef value,
                // the parameter was already passed (positionally or by a prior named arg).
                let existing = unsafe { &*(*call).cv(cv_idx) };
                if !existing.is_undef() {
                    let err = make_error_value("Error", &format!(
                        "Named parameter ${} overwrites previous argument", name
                    ));
                    // SAFETY: `call` is the non-null pending call owned by this live frame;
                    // the duplicate-argument path consumes and retires it exactly once.
                    match unsafe { cleanup_call_and_throw(eg, frame, call, err) }? {
                        ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                        ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                    }
                }

                let is_ref = func_common.sig.is_param_by_ref(idx);

                if is_ref {
                    // By-reference: same logic as SendRef
                    let argument = unsafe {
                        if opline._pad & SEND_FLAG_NONREFERENCEABLE != 0
                            && !func_common.sig.is_param_prefer_ref(idx)
                        {
                            let error = named_argument_reference_error(eg, func_common, idx);
                            return Ok(match cleanup_call_and_throw(eg, frame, call, error)? {
                                ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                            });
                        }
                        let reference_source = if yield_snapshot {
                            debug_assert_eq!(opline.result_type, OpType::Unused);
                            let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                            Some(base.add(opline.result as usize))
                        } else if opline.op1_type == OpType::Cv {
                            let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                            Some(base.add(opline.op1 as usize))
                        } else if matches!(opline.op1_type, OpType::Tmp | OpType::Var) {
                            let source =
                                (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                            (*source).is_reference().then_some(source)
                        } else {
                            None
                        };
                        if let Some(source) = reference_source {
                            materialize_reference_alias(frame, source)
                        } else if !func_common.sig.is_param_prefer_ref(idx) {
                            let error = named_argument_reference_error(eg, func_common, idx);
                            return Ok(match cleanup_call_and_throw(eg, frame, call, error)? {
                                ThrowResult::Handled(nf, no) => ColdResult::NewFrame(nf, no),
                                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                            });
                        } else {
                            snapshot_runtime_send_rvalue(eg, frame, op_array, opline)?
                        }
                    };
                    let arg_slot = unsafe { (*call).cv_mut(cv_idx) };
                    unsafe { frame_slot_init(call, arg_slot as *mut Value, argument) };
                } else {
                    // By-value: same logic as SendVal
                    let cloned = snapshot_runtime_send_rvalue(eg, frame, op_array, opline)?;
                    if let Some(exception) = eg.exception.take() {
                        // SAFETY: `call` is the non-null pending call owned by this live frame;
                        // propagating the value-snapshot exception consumes it exactly once.
                        match unsafe { cleanup_call_and_throw(eg, frame, call, exception) }? {
                            ThrowResult::Handled(nf, no) => {
                                return Ok(ColdResult::NewFrame(nf, no));
                            }
                            ThrowResult::Unhandled(t) => {
                                return Ok(ColdResult::Unhandled(t));
                            }
                        }
                    }
                    let arg_slot = unsafe { (*call).cv_mut(cv_idx) };
                    unsafe { frame_slot_init(call, arg_slot as *mut Value, cloned) };
                }

                // Update num_args to cover this position
                let public_pos = idx + 1; // 1-based count
                unsafe {
                    if (*call).num_args < public_pos {
                        (*call).num_args = public_pos;
                    }
                }
            }
            None => {
                let err = make_error_value("Error", &format!(
                    "Unknown named parameter ${}", name
                ));
                // SAFETY: `call` is the non-null pending call owned by this live frame;
                // the unknown-argument path consumes and retires it exactly once.
                match unsafe { cleanup_call_and_throw(eg, frame, call, err) }? {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }
    }
    Ok(ColdResult::Done)
}
