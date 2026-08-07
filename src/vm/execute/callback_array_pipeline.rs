use super::callback_pipeline::{CallbackArrayPipelineOrder, CallbackArrayPipelineProgram};

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
    let callback_name = |send: Instruction| {
        caller_op_array
            .literals
            .get(send.op1 as usize)
            .and_then(Value::as_str)
    };
    let map_func = callback_name(span.map_callback).and_then(|name| eg.find_function(name))?;
    let filter_func = callback_name(span.filter_callback).and_then(|name| eg.find_function(name))?;
    let reduce_func = callback_name(span.reduce_callback).and_then(|name| eg.find_function(name))?;

    let map_callback = prepare_scalar_long_callback(map_func, 1)?;
    let filter_callback = prepare_scalar_long_callback(filter_func, 1)?;
    let reduce_callback = prepare_scalar_long_callback(reduce_func, 2)?;

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
unsafe fn try_execute_filter_map_callback_array_pipeline(
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
