use super::callback_pipeline::{
    CallbackArrayPipelineOrder, CallbackArrayPipelineProgram, CallbackArrayPipelineSpan,
};

/// Fully guarded inputs for one scalar callback collection program. Raw
/// pointers are request-stable and are consumed synchronously before the
/// caller frame or source Value can change.
#[derive(Clone, Copy)]
struct PreparedCallbackArrayPipeline {
    source: *const PhpArray,
    map_callback: ScalarLongCallback,
    filter_callback: ScalarLongCallback,
    reduce_callback: ScalarLongCallback,
    initial: i64,
}

/// Staged destinations are inspected without reference dereferencing. A
/// parameter can expose even an Undef assignment to its caller and must then
/// retain the canonical materializing path.
#[inline(always)]
unsafe fn callback_pipeline_destinations_are_pristine(
    caller: *mut ExecuteData,
    discarded_cvs: Option<(u16, u16)>,
) -> bool {
    let Some((first_cv, second_cv)) = discarded_cvs else {
        return true;
    };
    let first = (*caller).cv(first_cv as u32);
    let second = (*caller).cv(second_cv as u32);
    first.value_type() == ValueType::Undef
        && !first.is_reference()
        && second.value_type() == ValueType::Undef
        && !second.is_reference()
}

/// Resolve invariant callback identities, pure scalar plans, source and carry
/// once. Any failure leaves the original bytecode untouched and replayable.
#[inline(always)]
unsafe fn prepare_callback_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    program: CallbackArrayPipelineProgram,
) -> Option<PreparedCallbackArrayPipeline> {
    if !callback_pipeline_destinations_are_pristine(caller, program.discarded_cvs) {
        return None;
    }
    let span = program.span;
    let prepare_callback = |send: Instruction, do_fcall_ip: usize, public_num_args: usize| {
        let callback = caller_op_array.literals.get(send.op1 as usize)?;
        let cache = caller_op_array.cache.as_ptr().add(do_fcall_ip)
            as *mut crate::vm::instruction::InlineCache;
        let func_ptr =
            crate::stdlib::resolve_literal_string_callback_with_cache(callback, eg, cache)?;
        prepare_scalar_long_callback(func_ptr, public_num_args)
    };
    let map_callback = prepare_callback(span.map_callback, span.map_do_fcall_ip, 1)?;
    let filter_callback =
        prepare_callback(span.filter_callback, span.filter_do_fcall_ip, 1)?;
    let reduce_callback = prepare_callback(span.reduce_callback, span.do_fcall_ip, 2)?;

    let source_value = &*(*caller).get_op_ptr(
        span.source.op1 as u32,
        span.source.op1_type,
        caller_op_array,
    );
    if source_value.is_reference() {
        return None;
    }
    let source = source_value.as_array()? as *const PhpArray;

    let initial = &*(*caller).get_op_ptr(
        span.initial.op1 as u32,
        span.initial.op1_type,
        caller_op_array,
    );
    if initial.value_type() != ValueType::Long || initial.is_reference() {
        return None;
    }

    Some(PreparedCallbackArrayPipeline {
        source,
        map_callback,
        filter_callback,
        reduce_callback,
        initial: initial.raw_long(),
    })
}

/// Execute one of the two monomorphic stage orders. `FILTER_FIRST` is compiled
/// out, so the member loop contains neither an enum match nor an indirect
/// stage dispatch.
#[inline(never)]
unsafe fn evaluate_callback_array_pipeline<const FILTER_FIRST: bool>(
    eg: &ExecutorGlobals,
    prepared: &PreparedCallbackArrayPipeline,
) -> Result<Option<i64>, VmError> {
    let source = &*prepared.source;
    let mut carry = prepared.initial;
    let mut retained_count = 0u64;

    for (index, value) in source.values().enumerate() {
        if index & 255 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            handle_interrupt(eg)?;
        }
        if value.value_type() != ValueType::Long || value.is_reference() {
            return Ok(None);
        }
        let input = value.raw_long();
        let mapped = if FILTER_FIRST {
            let keep = match prepared.filter_callback.evaluate_longs(&[input]) {
                Some(value) => value != 0,
                None => return Ok(None),
            };
            if !keep {
                continue;
            }
            retained_count += 1;
            match prepared.map_callback.evaluate_longs(&[input]) {
                Some(value) => value,
                None => return Ok(None),
            }
        } else {
            let mapped = match prepared.map_callback.evaluate_longs(&[input]) {
                Some(value) => value,
                None => return Ok(None),
            };
            let keep = match prepared.filter_callback.evaluate_longs(&[mapped]) {
                Some(value) => value != 0,
                None => return Ok(None),
            };
            if !keep {
                continue;
            }
            retained_count += 1;
            mapped
        };

        carry = match prepared.reduce_callback.evaluate_longs(&[carry, mapped]) {
            Some(value) => value,
            None => return Ok(None),
        };
    }

    let member_count = source.len() as u64;
    if FILTER_FIRST {
        prepared.filter_callback.record_calls(member_count);
        prepared.map_callback.record_calls(retained_count);
    } else {
        prepared.map_callback.record_calls(member_count);
        prepared.filter_callback.record_calls(member_count);
    }
    prepared.reduce_callback.record_calls(retained_count);
    Ok(Some(carry))
}

/// Dispatch the normalized program once, outside the member loop.
#[inline(always)]
unsafe fn execute_callback_array_pipeline_program(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    program: CallbackArrayPipelineProgram,
) -> Result<Option<(i64, *const Instruction)>, VmError> {
    let Some(prepared) = prepare_callback_array_pipeline(eg, caller, caller_op_array, program)
    else {
        return Ok(None);
    };
    let result = match program.order {
        CallbackArrayPipelineOrder::MapFilter => {
            evaluate_callback_array_pipeline::<false>(eg, &prepared)?
        }
        CallbackArrayPipelineOrder::FilterMap => {
            evaluate_callback_array_pipeline::<true>(eg, &prepared)?
        }
    };
    Ok(result.map(|carry| {
        (
            carry,
            caller_op_array
                .instructions
                .as_ptr()
                .add(program.span.do_fcall_ip),
        )
    }))
}

/// Execute an exact nested map/filter/reduce span as one streaming pass.
#[inline(never)]
unsafe fn try_execute_uncached_callback_array_pipeline(
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
    let entry_ip = reduce_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    let Some(span) = crate::vm::callback_pipeline::detect_callback_array_pipeline_span(
        caller_op_array,
        entry_ip,
    ) else {
        return Ok(None);
    };
    execute_callback_array_pipeline_program(
        eg,
        caller,
        caller_op_array,
        CallbackArrayPipelineProgram {
            span,
            order: CallbackArrayPipelineOrder::MapFilter,
            discarded_cvs: None,
        },
    )
}

/// Keep staged entry guards separate while sharing the normalized program.
#[inline(never)]
unsafe fn try_execute_uncached_staged_callback_array_pipeline(
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
    let entry_ip = map_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    let Some(staged) =
        crate::vm::callback_pipeline::detect_staged_callback_array_pipeline_span(
            caller_op_array,
            entry_ip,
        )
    else {
        return Ok(None);
    };
    execute_callback_array_pipeline_program(
        eg,
        caller,
        caller_op_array,
        CallbackArrayPipelineProgram {
            span: staged.pipeline,
            order: CallbackArrayPipelineOrder::MapFilter,
            discarded_cvs: Some((staged.mapped_cv, staged.filtered_cv)),
        },
    )
}

/// Execute nested or dead-staged filter/map/reduce through the same program.
#[inline(never)]
unsafe fn try_execute_uncached_filter_map_callback_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    pipeline_ptr: *const Instruction,
) -> Result<Option<(i64, *const Instruction)>, VmError> {
    let pipeline = &*pipeline_ptr;
    if pipeline.opcode != OpCode::InitFcall
        || pipeline._pad & CALL_FLAG_FILTER_MAP_CALLBACK_ARRAY_PIPELINE == 0
    {
        return Ok(None);
    }
    let entry_ip = pipeline_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    let Some(detected) =
        crate::vm::callback_pipeline::detect_filter_map_callback_array_pipeline_span(
            caller_op_array,
            entry_ip,
        )
    else {
        return Ok(None);
    };
    execute_callback_array_pipeline_program(
        eg,
        caller,
        caller_op_array,
        CallbackArrayPipelineProgram {
            span: detected.pipeline,
            order: CallbackArrayPipelineOrder::FilterMap,
            discarded_cvs: detected.discarded_cvs,
        },
    )
}

/// Decode compiler-proven JSON sink metadata without re-validating immutable
/// bytecode on every execution. Runtime data, callback and builtin guards stay
/// authoritative and can still replay the untouched canonical instructions.
#[inline(always)]
unsafe fn prepared_json_callback_array_pipeline_program(
    caller_op_array: &crate::compiler::OpArray,
    entry_ip: usize,
    entry: &Instruction,
) -> Option<(CallbackArrayPipelineProgram, usize, usize)> {
    let filter_first = entry._pad & CALL_FLAG_CALLBACK_ARRAY_PIPELINE_FILTER_FIRST != 0;
    let staged = entry._pad & CALL_FLAG_CALLBACK_ARRAY_PIPELINE_STAGED_METADATA != 0;
    let instruction = |offset: usize| {
        caller_op_array
            .instructions
            .get(entry_ip + offset)
            .copied()
    };
    let (
        map_callback,
        map_do_fcall_ip,
        source,
        filter_callback,
        filter_do_fcall_ip,
        reduce_callback,
        initial,
    ) =
        match (staged, filter_first) {
            (false, false) => (
                instruction(4)?,
                entry_ip + 6,
                instruction(5)?,
                instruction(8)?,
                entry_ip + 9,
                instruction(11)?,
                instruction(12)?,
            ),
            (false, true) => (
                instruction(3)?,
                entry_ip + 9,
                instruction(5)?,
                instruction(6)?,
                entry_ip + 7,
                instruction(11)?,
                instruction(12)?,
            ),
            (true, false) => (
                instruction(1)?,
                entry_ip + 3,
                instruction(2)?,
                instruction(7)?,
                entry_ip + 8,
                instruction(13)?,
                instruction(14)?,
            ),
            (true, true) => (
                instruction(6)?,
                entry_ip + 8,
                instruction(1)?,
                instruction(2)?,
                entry_ip + 3,
                instruction(13)?,
                instruction(14)?,
            ),
        };
    let discarded_cvs = if staged {
        Some((instruction(4)?.op1, instruction(9)?.op1))
    } else {
        None
    };
    let reduce_do_ip = entry_ip + if staged { 15 } else { 13 };
    let json_init_ip = entry_ip + if staged { 10 } else { 0 };
    let json_do_ip = entry_ip + if staged { 17 } else { 15 };
    caller_op_array.instructions.get(json_do_ip)?;
    Some((
        CallbackArrayPipelineProgram {
            span: CallbackArrayPipelineSpan {
                map_callback,
                map_do_fcall_ip,
                source,
                filter_callback,
                filter_do_fcall_ip,
                reduce_callback,
                initial,
                do_fcall_ip: reduce_do_ip,
            },
            order: if filter_first {
                CallbackArrayPipelineOrder::FilterMap
            } else {
                CallbackArrayPipelineOrder::MapFilter
            },
            discarded_cvs,
        },
        json_init_ip,
        json_do_ip,
    ))
}

/// Execute an exact `json_encode(Long callback-pipeline aggregate)` wrapper.
/// Long JSON has no escaping or allocation-sensitive failure cases, so the
/// inner temporary can be omitted and only the final String is materialized.
#[inline(never)]
unsafe fn try_execute_json_callback_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    json_ptr: *const Instruction,
) -> Result<Option<(String, *const Instruction)>, VmError> {
    let json = &*json_ptr;
    if json.opcode != OpCode::InitFcall
        || json._pad & CALL_FLAG_CALLBACK_ARRAY_PIPELINE_JSON_SINK == 0
    {
        return Ok(None);
    }

    let entry_ip = json_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    debug_assert!(
        crate::vm::callback_pipeline::detect_json_callback_array_pipeline_span(
            caller_op_array,
            entry_ip,
        )
        .is_some()
    );
    let Some((program, json_init_ip, json_do_ip)) =
        prepared_json_callback_array_pipeline_program(caller_op_array, entry_ip, json)
    else {
        return Ok(None);
    };
    let cache = caller_op_array.cache.as_ptr().add(json_init_ip);
    let mut json_func = (*cache).func;
    if json_func.is_null() {
        let Some(resolved) = eg.find_function("json_encode") else {
            return Ok(None);
        };
        json_func = resolved;
        (*(cache as *mut crate::vm::instruction::InlineCache)).func = resolved;
    }
    let json_common = &*json_func;
    if json_common.fn_type != FunctionType::Internal
        || json_common.sig.required_num_args != 1
        || json_common.sig.public_arity() != 1
        || json_common.sig.ref_args != 0
    {
        return Ok(None);
    }
    let Some(prepared) = prepare_callback_array_pipeline(eg, caller, caller_op_array, program) else {
        return Ok(None);
    };
    let result = match program.order {
        CallbackArrayPipelineOrder::MapFilter => {
            evaluate_callback_array_pipeline::<false>(eg, &prepared)?
        }
        CallbackArrayPipelineOrder::FilterMap => {
            evaluate_callback_array_pipeline::<true>(eg, &prepared)?
        }
    };
    Ok(result.map(|value| {
        (
            value.to_string(),
            caller_op_array.instructions.as_ptr().add(json_do_ip),
        )
    }))
}

/// Decode immutable compiler metadata after this call site has completed the
/// full structural detector once. The four admitted layouts are fixed by the
/// same compiler pass that sets their entry marker.
#[inline(always)]
unsafe fn callback_array_pipeline_program_from_metadata(
    caller_op_array: &crate::compiler::OpArray,
    entry_ip: usize,
    order: CallbackArrayPipelineOrder,
    staged: bool,
) -> Option<CallbackArrayPipelineProgram> {
    let instruction = |offset: usize| {
        caller_op_array
            .instructions
            .get(entry_ip + offset)
            .copied()
    };
    let (
        map_callback,
        map_do_fcall_ip,
        source,
        filter_callback,
        filter_do_fcall_ip,
        reduce_callback,
        initial,
        do_fcall_ip,
    ) =
        match (staged, order) {
            (false, CallbackArrayPipelineOrder::MapFilter) => (
                instruction(3)?,
                entry_ip + 5,
                instruction(4)?,
                instruction(7)?,
                entry_ip + 8,
                instruction(10)?,
                instruction(11)?,
                entry_ip + 12,
            ),
            (false, CallbackArrayPipelineOrder::FilterMap) => (
                instruction(2)?,
                entry_ip + 8,
                instruction(4)?,
                instruction(5)?,
                entry_ip + 6,
                instruction(10)?,
                instruction(11)?,
                entry_ip + 12,
            ),
            (true, CallbackArrayPipelineOrder::MapFilter) => (
                instruction(1)?,
                entry_ip + 3,
                instruction(2)?,
                instruction(7)?,
                entry_ip + 8,
                instruction(12)?,
                instruction(13)?,
                entry_ip + 14,
            ),
            (true, CallbackArrayPipelineOrder::FilterMap) => (
                instruction(6)?,
                entry_ip + 8,
                instruction(1)?,
                instruction(2)?,
                entry_ip + 3,
                instruction(12)?,
                instruction(13)?,
                entry_ip + 14,
            ),
        };
    caller_op_array.instructions.get(do_fcall_ip)?;
    let discarded_cvs = if staged {
        Some((instruction(4)?.op1, instruction(9)?.op1))
    } else {
        None
    };
    Some(CallbackArrayPipelineProgram {
        span: CallbackArrayPipelineSpan {
            map_callback,
            map_do_fcall_ip,
            source,
            filter_callback,
            filter_do_fcall_ip,
            reduce_callback,
            initial,
            do_fcall_ip,
        },
        order,
        discarded_cvs,
    })
}

#[inline(always)]
unsafe fn callback_array_pipeline_metadata_is_armed(
    caller_op_array: &crate::compiler::OpArray,
    entry_ip: usize,
) -> bool {
    (*caller_op_array.cache.as_ptr().add(entry_ip)).callback_pipeline_metadata_armed()
}

#[inline(always)]
unsafe fn arm_callback_array_pipeline_metadata(
    caller_op_array: &crate::compiler::OpArray,
    entry_ip: usize,
) {
    (*(caller_op_array.cache.as_ptr().add(entry_ip)
        as *mut crate::vm::instruction::InlineCache))
        .arm_callback_pipeline_metadata();
}

/// The first successful execution retains the canonical structural detector;
/// later executions at the same call site consume compiler-proven metadata.
#[inline(never)]
unsafe fn try_execute_callback_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    reduce_ptr: *const Instruction,
) -> Result<Option<(i64, *const Instruction)>, VmError> {
    let entry_ip = reduce_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    if callback_array_pipeline_metadata_is_armed(caller_op_array, entry_ip) {
        let Some(program) = callback_array_pipeline_program_from_metadata(
            caller_op_array,
            entry_ip,
            CallbackArrayPipelineOrder::MapFilter,
            false,
        ) else {
            return Ok(None);
        };
        return execute_callback_array_pipeline_program(eg, caller, caller_op_array, program);
    }
    let result = try_execute_uncached_callback_array_pipeline(
        eg,
        caller,
        caller_op_array,
        reduce_ptr,
    )?;
    if result.is_some() {
        arm_callback_array_pipeline_metadata(caller_op_array, entry_ip);
    }
    Ok(result)
}

#[inline(never)]
unsafe fn try_execute_staged_callback_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    map_ptr: *const Instruction,
) -> Result<Option<(i64, *const Instruction)>, VmError> {
    let entry_ip = map_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    if callback_array_pipeline_metadata_is_armed(caller_op_array, entry_ip) {
        let Some(program) = callback_array_pipeline_program_from_metadata(
            caller_op_array,
            entry_ip,
            CallbackArrayPipelineOrder::MapFilter,
            true,
        ) else {
            return Ok(None);
        };
        return execute_callback_array_pipeline_program(eg, caller, caller_op_array, program);
    }
    let result = try_execute_uncached_staged_callback_array_pipeline(
        eg,
        caller,
        caller_op_array,
        map_ptr,
    )?;
    if result.is_some() {
        arm_callback_array_pipeline_metadata(caller_op_array, entry_ip);
    }
    Ok(result)
}

#[inline(never)]
unsafe fn try_execute_filter_map_callback_array_pipeline(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    pipeline_ptr: *const Instruction,
) -> Result<Option<(i64, *const Instruction)>, VmError> {
    let entry_ip = pipeline_ptr.offset_from(caller_op_array.instructions.as_ptr()) as usize;
    if callback_array_pipeline_metadata_is_armed(caller_op_array, entry_ip) {
        let staged = (*pipeline_ptr)._pad & CALL_FLAG_CALLBACK_ARRAY_PIPELINE_STAGED_METADATA != 0;
        let Some(program) = callback_array_pipeline_program_from_metadata(
            caller_op_array,
            entry_ip,
            CallbackArrayPipelineOrder::FilterMap,
            staged,
        ) else {
            return Ok(None);
        };
        return execute_callback_array_pipeline_program(eg, caller, caller_op_array, program);
    }
    let result = try_execute_uncached_filter_map_callback_array_pipeline(
        eg,
        caller,
        caller_op_array,
        pipeline_ptr,
    )?;
    if result.is_some() {
        arm_callback_array_pipeline_metadata(caller_op_array, entry_ip);
    }
    Ok(result)
}
