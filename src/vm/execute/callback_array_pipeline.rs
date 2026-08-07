/// Execute an exact nested map/filter/reduce span as one streaming pass. Only
/// compiler-proven pure scalar callback plans are admitted, so any runtime
/// guard failure may replay the untouched canonical bytecode safely.
#[inline(never)]
unsafe fn try_execute_callback_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    reduce_ptr: *const Instruction,
) -> Result<Option<(i64, *const Instruction)>, VmError> {
    let reduce = &*reduce_ptr;
    if reduce.opcode != OpCode::InitFcall
        || reduce._pad & CALL_FLAG_CALLBACK_ARRAY_PIPELINE == 0
    {
        return Ok(None);
    }

    let reduce_ip = reduce_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    let Some(span) = crate::vm::callback_pipeline::detect_callback_array_pipeline_span(
        caller_op_array,
        reduce_ip,
    ) else {
        return Ok(None);
    };

    let callback_name = |send: Instruction| {
        caller_op_array
            .literals
            .get(send.op1 as usize)
            .and_then(Value::as_str)
    };
    let Some(map_func) = callback_name(span.map_callback).and_then(|name| eg.find_function(name))
    else {
        return Ok(None);
    };
    let Some(filter_func) = callback_name(span.filter_callback)
        .and_then(|name| eg.find_function(name))
    else {
        return Ok(None);
    };
    let Some(reduce_func) = callback_name(span.reduce_callback)
        .and_then(|name| eg.find_function(name))
    else {
        return Ok(None);
    };

    let Some(map_callback) = prepare_scalar_long_callback(map_func, 1) else {
        return Ok(None);
    };
    let Some(filter_callback) = prepare_scalar_long_callback(filter_func, 1) else {
        return Ok(None);
    };
    let Some(reduce_callback) = prepare_scalar_long_callback(reduce_func, 2) else {
        return Ok(None);
    };

    let source = &*(*caller).get_op_ptr(
        span.source.op1 as u32,
        span.source.op1_type,
        caller_op_array,
    );
    if source.is_reference() {
        return Ok(None);
    }
    let Some(source) = source.as_array() else {
        return Ok(None);
    };
    let initial = &*(*caller).get_op_ptr(
        span.initial.op1 as u32,
        span.initial.op1_type,
        caller_op_array,
    );
    if initial.value_type() != ValueType::Long || initial.is_reference() {
        return Ok(None);
    }

    let mut carry = initial.raw_long();
    let mut reduce_calls = 0u64;
    for (index, value) in source.values().enumerate() {
        if index & 255 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            handle_interrupt(eg)?;
        }
        if value.value_type() != ValueType::Long || value.is_reference() {
            return Ok(None);
        }
        let mapped = match map_callback.evaluate_longs(&[value.raw_long()]) {
            Some(value) => value,
            None => return Ok(None),
        };
        let keep = match filter_callback.evaluate_longs(&[mapped]) {
            Some(value) => value != 0,
            None => return Ok(None),
        };
        if keep {
            carry = match reduce_callback.evaluate_longs(&[carry, mapped]) {
                Some(value) => value,
                None => return Ok(None),
            };
            reduce_calls += 1;
        }
    }

    let member_count = source.len() as u64;
    map_callback.record_calls(member_count);
    filter_callback.record_calls(member_count);
    reduce_callback.record_calls(reduce_calls);
    Ok(Some((
        carry,
        caller_op_array.instructions.as_ptr().add(span.do_fcall_ip),
    )))
}

/// Keep staged fusion in a separate entry so extending its escape guards does
/// not perturb code generation for the already-tuned nested hot path above.
#[inline(never)]
unsafe fn try_execute_staged_callback_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    map_ptr: *const Instruction,
) -> Result<Option<(i64, *const Instruction)>, VmError> {
    let map = &*map_ptr;
    if map.opcode != OpCode::InitFcall
        || map._pad & CALL_FLAG_STAGED_CALLBACK_ARRAY_PIPELINE == 0
    {
        return Ok(None);
    }

    let map_ip = map_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    let Some(staged) =
        crate::vm::callback_pipeline::detect_staged_callback_array_pipeline_span(
            caller_op_array,
            map_ip,
        )
    else {
        return Ok(None);
    };

    // Inspect raw CV slots because get_op_ptr() deliberately dereferences
    // aliases. Parameters have no defining opcode, and a reference parameter
    // can expose even an Undef assignment to its caller.
    let mapped = (*caller).cv(staged.mapped_cv as u32);
    let filtered = (*caller).cv(staged.filtered_cv as u32);
    if mapped.value_type() != ValueType::Undef
        || mapped.is_reference()
        || filtered.value_type() != ValueType::Undef
        || filtered.is_reference()
    {
        return Ok(None);
    }
    let span = staged.pipeline;

    let callback_name = |send: Instruction| {
        caller_op_array
            .literals
            .get(send.op1 as usize)
            .and_then(Value::as_str)
    };
    let Some(map_func) = callback_name(span.map_callback).and_then(|name| eg.find_function(name))
    else {
        return Ok(None);
    };
    let Some(filter_func) = callback_name(span.filter_callback)
        .and_then(|name| eg.find_function(name))
    else {
        return Ok(None);
    };
    let Some(reduce_func) = callback_name(span.reduce_callback)
        .and_then(|name| eg.find_function(name))
    else {
        return Ok(None);
    };

    let Some(map_callback) = prepare_scalar_long_callback(map_func, 1) else {
        return Ok(None);
    };
    let Some(filter_callback) = prepare_scalar_long_callback(filter_func, 1) else {
        return Ok(None);
    };
    let Some(reduce_callback) = prepare_scalar_long_callback(reduce_func, 2) else {
        return Ok(None);
    };

    let source = &*(*caller).get_op_ptr(
        span.source.op1 as u32,
        span.source.op1_type,
        caller_op_array,
    );
    if source.is_reference() {
        return Ok(None);
    }
    let Some(source) = source.as_array() else {
        return Ok(None);
    };
    let initial = &*(*caller).get_op_ptr(
        span.initial.op1 as u32,
        span.initial.op1_type,
        caller_op_array,
    );
    if initial.value_type() != ValueType::Long || initial.is_reference() {
        return Ok(None);
    }

    let mut carry = initial.raw_long();
    let mut reduce_calls = 0u64;
    for (index, value) in source.values().enumerate() {
        if index & 255 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            handle_interrupt(eg)?;
        }
        if value.value_type() != ValueType::Long || value.is_reference() {
            return Ok(None);
        }
        let mapped = match map_callback.evaluate_longs(&[value.raw_long()]) {
            Some(value) => value,
            None => return Ok(None),
        };
        let keep = match filter_callback.evaluate_longs(&[mapped]) {
            Some(value) => value != 0,
            None => return Ok(None),
        };
        if keep {
            carry = match reduce_callback.evaluate_longs(&[carry, mapped]) {
                Some(value) => value,
                None => return Ok(None),
            };
            reduce_calls += 1;
        }
    }

    let member_count = source.len() as u64;
    map_callback.record_calls(member_count);
    filter_callback.record_calls(member_count);
    reduce_callback.record_calls(reduce_calls);
    Ok(Some((
        carry,
        caller_op_array.instructions.as_ptr().add(span.do_fcall_ip),
    )))
}
