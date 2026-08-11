// Kept in the execute module through include! so this structural split does not change visibility or code generation.

fn native_mixed_string_mask_before(
    kernel: &NativeQuickLongMixedKernel,
    before_operation: u8,
) -> u64 {
    kernel
        .config
        .operations
        .iter()
        .copied()
        .take(before_operation as usize)
        .fold(0u64, |mask, operation| match operation {
            NativeStraightLongOperation::StringToken { result, .. }
            | NativeStraightLongOperation::Move { result, .. }
                if kernel.string_output_mask & (1u64 << result) != 0 =>
            {
                mask | (1u64 << result)
            }
            _ => mask,
        })
}

fn publish_native_mixed_trace_guards(
    kernel: &NativeQuickLongMixedKernel,
    slots: &mut [i64; 64],
    dirty_bool_mask: &mut u64,
    before_operation: Option<u8>,
) {
    for index in 0..kernel.trace_guard_count as usize {
        if before_operation.is_some_and(|limit| {
            kernel.trace_guard_operation_indices[index] >= limit
        }) {
            continue;
        }
        let slot = kernel.trace_guard_condition_slots[index] as usize;
        slots[slot] = i64::from(kernel.trace_guard_expected[index]);
        *dirty_bool_mask |= 1u64 << slot;
    }
}

unsafe fn publish_native_mixed_strings(
    op_array: &crate::compiler::OpArray,
    kernel: &NativeQuickLongMixedKernel,
    slots: &[i64; 64],
    string_state: &mut QuickStringSlotState,
    mut mask: u64,
) -> Option<()> {
    while mask != 0 {
        let slot = mask.trailing_zeros() as usize;
        mask &= mask - 1;
        let token = usize::try_from(slots[slot]).ok()?;
        if token >= kernel.string_token_count as usize {
            return None;
        }
        let literal = kernel.string_literals[token] as usize;
        let value = op_array.literals.get(literal)? as *const Value;
        string_state.assign_literal(slot as u16, value);
    }
    Some(())
}

fn record_native_mixed_calls(
    kernel: &NativeQuickLongMixedKernel,
    completed_iterations: u64,
    completed_current_before: Option<u8>,
) {
    for index in 0..kernel.call_count as usize {
        let current = u64::from(completed_current_before.is_some_and(|failed| {
            kernel.call_completion_operations[index] < failed
        }));
        let count = completed_iterations.saturating_add(current);
        if count != 0 {
            unsafe { record_scalar_calls_bulk(&*kernel.call_targets[index], count) };
        }
    }
}

fn native_mixed_array_reserve_hint(remaining_iterations: u64, array_count: u32) -> usize {
    if remaining_iterations < NATIVE_ARRAY_RESERVE_MIN_ITERATIONS {
        return 0;
    }
    let Ok(array_count) = usize::try_from(array_count) else {
        return 0;
    };
    if array_count == 0 {
        return 0;
    }
    usize::try_from(remaining_iterations)
        .unwrap_or(usize::MAX)
        .min(NATIVE_ARRAY_RESERVE_ENTRY_BUDGET / array_count)
}

fn native_mixed_array_write_config(
    kernel: &NativeQuickLongMixedKernel,
    deferred_reserve: bool,
) -> NativeStraightLongLoopConfig {
    let mut config = kernel.config;
    for operation in config
        .operations
        .iter_mut()
        .take(config.operation_count as usize)
    {
        if let NativeStraightLongOperation::ArrayLongSet {
            deferred_reserve: operation_deferred_reserve,
            ..
        } = operation
        {
            *operation_deferred_reserve = deferred_reserve;
        }
    }
    config
}

unsafe fn prepare_native_mixed_properties(
    kernel: &NativeQuickLongMixedKernel,
    resolved_object_ops: &[QuickResolvedObjectOp],
    slots: &mut [i64; 64],
) -> Option<[*mut Value; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES]> {
    let mut values = [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    for index in 0..kernel.property_binding_count as usize {
        let op_index = kernel.property_binding_op_indices[index] as usize;
        let property_index = kernel.property_binding_property_indices[index] as usize;
        let (receiver, property_slot) = match *resolved_object_ops.get(op_index)? {
            QuickResolvedObjectOp::PropertyMethod {
                receiver,
                property_slots,
                property_count,
                ..
            } => {
                if property_index >= property_count as usize {
                    return None;
                }
                (receiver, property_slots[property_index])
            }
            QuickResolvedObjectOp::PropertyGetter {
                receiver,
                property_slot,
                ..
            } if property_index == 0 => (receiver, property_slot),
            _ => return None,
        };
        if receiver.is_null() {
            return None;
        }
        let value = (*receiver).object_property_slot_unchecked(property_slot) as *mut Value;
        if value.is_null()
            || (*value).value_type() != ValueType::Long
            || (*value).is_reference()
        {
            return None;
        }
        let shadow_slot = kernel.property_binding_slots[index] as usize;
        slots[shadow_slot] = (*value).raw_long();
        values[index] = value;
    }
    Some(values)
}

unsafe fn commit_native_mixed_properties(
    kernel: &NativeQuickLongMixedKernel,
    properties: &[*mut Value; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    slots: &[i64; 64],
) {
    for index in 0..kernel.property_binding_count as usize {
        Value::write_long(
            properties[index],
            slots[kernel.property_binding_slots[index] as usize],
        );
    }
}

#[inline(never)]
unsafe fn run_native_quick_long_mixed_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: &mut [i64; 64],
    mutable_arrays: &[*mut PhpArray; 64],
    string_state: &mut QuickStringSlotState,
    resolved_object_ops: &[QuickResolvedObjectOp],
    kernel: &NativeQuickLongMixedKernel,
) -> Result<Option<QuickLoopOutcome>, VmError> {
    for slot in 0..64usize {
        if plan.string_input_mask & (1u64 << slot) == 0 {
            continue;
        }
        let value = string_state.value(slot as u16).as_str().unwrap_unchecked();
        let token = (0..kernel.string_token_count as usize).find(|token| {
            op_array.literals[kernel.string_literals[*token] as usize]
                .as_str()
                .is_some_and(|literal| literal == value)
        });
        let Some(token) = token else {
            return Ok(None);
        };
        slots[slot] = token as i64;
    }

    let Some(property_values) = prepare_native_mixed_properties(
        kernel,
        resolved_object_ops,
        slots,
    ) else {
        return Ok(None);
    };

    let bound = quick_long_operand(slots, kernel.config.bound);
    let induction = slots[kernel.config.induction_slot as usize];
    let remaining_iterations = (induction < bound)
        .then(|| (bound as u64).wrapping_sub(induction as u64))
        .unwrap_or(0);
    let mut mutable_array_mask = 0u64;
    for index in 0..kernel.context_count as usize {
        if kernel.context_kinds[index] == NativeMixedContextKind::MutableArray {
            mutable_array_mask |= 1u64 << kernel.context_array_slots[index];
        }
    }
    let reserve_hint = native_mixed_array_reserve_hint(
        remaining_iterations,
        mutable_array_mask.count_ones(),
    );
    let mut deferred_array_writes = false;
    for index in 0..kernel.context_count as usize {
        if kernel.context_kinds[index] != NativeMixedContextKind::MutableArray {
            continue;
        }
        let array = mutable_arrays[kernel.context_array_slots[index] as usize];
        if array.is_null() {
            return Ok(None);
        }
        deferred_array_writes |= reserve_hint != 0 && !(*array).can_reserve_indexed_int_writes();
    }
    // Keep exactly two cache identities: the common direct helper and one
    // alternate where every structural write uses a deferred context. This
    // avoids compiling a combinatorial set for loops with multiple arrays.
    let config = native_mixed_array_write_config(kernel, deferred_array_writes);

    let mut context_pointers =
        [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    let mut indexed_contexts = [
        std::mem::MaybeUninit::<NativeIndexedLongLookupContext>::uninit();
        NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES
    ];
    let mut array_set_contexts = [
        std::mem::MaybeUninit::<NativeLongArraySetContext>::uninit();
        NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES
    ];
    for index in 0..kernel.context_count as usize {
        match kernel.context_kinds[index] {
            NativeMixedContextKind::IndexedRead => {
                let Some(array) =
                    (*slot_base.add(kernel.context_array_slots[index] as usize)).as_array()
                else {
                    return Ok(None);
                };
                let Some(context) = array.native_indexed_long_lookup_context() else {
                    return Ok(None);
                };
                context_pointers[index] = indexed_contexts[index].write(context)
                    as *mut NativeIndexedLongLookupContext
                    as *mut i64;
            }
            NativeMixedContextKind::MutableArray => {
                let array = mutable_arrays[kernel.context_array_slots[index] as usize];
                if array.is_null() {
                    return Ok(None);
                }
                if deferred_array_writes {
                    let array_slot = kernel.context_array_slots[index];
                    let first_context_for_array = (0..index).all(|earlier| {
                        kernel.context_kinds[earlier] != NativeMixedContextKind::MutableArray
                            || kernel.context_array_slots[earlier] != array_slot
                    });
                    let context = array_set_contexts[index].write(NativeLongArraySetContext::new(
                        array,
                        if first_context_for_array {
                            reserve_hint
                        } else {
                            0
                        },
                    ));
                    context_pointers[index] =
                        (context as *mut NativeLongArraySetContext).cast::<i64>();
                } else {
                    context_pointers[index] = array.cast::<i64>();
                }
            }
            NativeMixedContextKind::Entry => {
                let array = mutable_arrays[kernel.context_array_slots[index] as usize];
                if array.is_null() {
                    return Ok(None);
                }
                let token = kernel.context_tokens[index] as usize;
                let key = op_array.literals[kernel.string_literals[token] as usize]
                    .as_str()
                    .unwrap_unchecked();
                let value = match canonical_decimal_array_key(key) {
                    Some(key) => (*array).get_int_mut(key),
                    None => (*array).get_str_mut(key),
                };
                let Some(value) = value else {
                    return Ok(None);
                };
                if value.value_type() != ValueType::Long || value.is_reference() {
                    return Ok(None);
                }
                context_pointers[index] = value as *mut Value as *mut i64;
            }
        }
    }

    let cache = plan.native_jit();
    let program = if deferred_array_writes {
        cache.prepare_alternate_straight_program(&config)
    } else {
        cache.prepare_straight_program(&config)
    };
    let Some(program) = program else {
        return Ok(None);
    };
    if !deferred_array_writes && reserve_hint != 0 {
        let mut arrays_to_reserve = mutable_array_mask;
        while arrays_to_reserve != 0 {
            let array_slot = arrays_to_reserve.trailing_zeros() as usize;
            arrays_to_reserve &= arrays_to_reserve - 1;
            let array = mutable_arrays[array_slot];
            debug_assert!(!array.is_null());
            let reserved = (*array).reserve_indexed_int_writes(reserve_hint);
            debug_assert!(reserved);
        }
    }
    let visible_body_output_mask = kernel.config.body_output_mask() & kernel.long_output_mask;
    let post_result_mask = kernel.config.post_result.map_or(0, |slot| 1u64 << slot);
    let mut iterations = 0u64;
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut entered_native = false;

    loop {
        let before_induction = slots[kernel.config.induction_slot as usize];
        let mut before_values = [0i64; NATIVE_QUICK_LONG_SLOT_CAPACITY];
        for index in 0..kernel.mutable_slot_count as usize {
            before_values[index] = slots[kernel.mutable_slots[index] as usize];
        }
        let mut before_entries = [0i64; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
        for index in 0..kernel.context_count as usize {
            if kernel.context_kinds[index] == NativeMixedContextKind::Entry {
                before_entries[index] = *context_pointers[index];
            }
        }

        let native_result = cache.dispatch_prepared_straight_chunk_with_context(
            program,
            slots,
            NATIVE_LONG_SAFEPOINT_INTERVAL,
            &context_pointers,
        );
        let mut result = match native_result {
            Ok(result) => {
                if !entered_native {
                    cache.record_region_entry();
                    entered_native = true;
                }
                result
            }
            Err(error) => {
                for index in 0..kernel.mutable_slot_count as usize {
                    slots[kernel.mutable_slots[index] as usize] = before_values[index];
                }
                for index in 0..kernel.context_count as usize {
                    if kernel.context_kinds[index] == NativeMixedContextKind::Entry {
                        *context_pointers[index] = before_entries[index];
                    }
                }
                commit_native_mixed_properties(kernel, &property_values, slots);
                return Err(VmError::Fatal(format!(
                    "native mixed-region dispatch failed: {:?}",
                    error
                )));
            }
        };

        let induction = slots[kernel.config.induction_slot as usize];
        let completed_in_chunk =
            (induction as u64).wrapping_sub(before_induction as u64);
        iterations = iterations.saturating_add(completed_in_chunk);
        if completed_in_chunk != 0 {
            dirty_long_mask |= (1u64 << kernel.config.induction_slot)
                | visible_body_output_mask
                | post_result_mask;
        }
        if result.outcome == NativeStraightLongLoopOutcome::ChunkExhausted
            && induction >= bound
        {
            result.outcome = NativeStraightLongLoopOutcome::Completed;
        }
        let completed = result.outcome == NativeStraightLongLoopOutcome::Completed;
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(!completed);
            dirty_bool_mask |= 1u64 << slot;
        }

        match result.outcome {
            NativeStraightLongLoopOutcome::Completed => {
                record_native_mixed_calls(kernel, completed_in_chunk, None);
                if iterations != 0 {
                    publish_native_mixed_trace_guards(
                        kernel,
                        slots,
                        &mut dirty_bool_mask,
                        None,
                    );
                    publish_native_mixed_strings(
                        op_array,
                        kernel,
                        slots,
                        string_state,
                        kernel.string_output_mask,
                    )
                    .expect("validated native String token");
                }
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                commit_native_mixed_properties(kernel, &property_values, slots);
                string_state.commit();
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.exit_target.exit_ip().unwrap_unchecked());
                stats::inc_quick_loop_completed(iterations);
                return Ok(Some(QuickLoopOutcome::Completed));
            }
            NativeStraightLongLoopOutcome::ChunkExhausted => {
                record_native_mixed_calls(kernel, completed_in_chunk, None);
                if eg.vm_interrupt.load(Ordering::Relaxed) {
                    if iterations != 0 {
                        publish_native_mixed_trace_guards(
                            kernel,
                            slots,
                            &mut dirty_bool_mask,
                            None,
                        );
                        publish_native_mixed_strings(
                            op_array,
                            kernel,
                            slots,
                            string_state,
                            kernel.string_output_mask,
                        )
                        .expect("validated native String token");
                    }
                    commit_quick_long_ops_slots(
                        slot_base,
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    commit_native_mixed_properties(kernel, &property_values, slots);
                    string_state.commit();
                    (*frame).opline = op_array.instructions.as_ptr().add(
                        plan.target_ip(kernel.body_target).unwrap_unchecked(),
                    );
                    handle_interrupt(eg)?;
                }
            }
            NativeStraightLongLoopOutcome::OperationSideExit => {
                let failed = result.failed_operation.expect("native mixed side-exit index");
                record_native_mixed_calls(kernel, completed_in_chunk, Some(failed));
                dirty_long_mask |=
                    kernel.config.output_mask_before(failed) & kernel.long_output_mask;
                publish_native_mixed_trace_guards(
                    kernel,
                    slots,
                    &mut dirty_bool_mask,
                    (iterations == 0).then_some(failed),
                );
                let string_mask = if iterations != 0 {
                    kernel.string_output_mask
                } else {
                    native_mixed_string_mask_before(kernel, failed)
                };
                publish_native_mixed_strings(
                    op_array,
                    kernel,
                    slots,
                    string_state,
                    string_mask,
                )
                .expect("validated native String token");
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                commit_native_mixed_properties(kernel, &property_values, slots);
                string_state.commit();
                (*frame).opline = op_array.instructions.as_ptr().add(
                    kernel.operation_resume_ips[failed as usize],
                );
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            NativeStraightLongLoopOutcome::IncrementOverflow => {
                record_native_mixed_calls(kernel, completed_in_chunk, Some(u8::MAX));
                dirty_long_mask |= visible_body_output_mask;
                publish_native_mixed_trace_guards(
                    kernel,
                    slots,
                    &mut dirty_bool_mask,
                    None,
                );
                publish_native_mixed_strings(
                    op_array,
                    kernel,
                    slots,
                    string_state,
                    kernel.string_output_mask,
                )
                .expect("validated native String token");
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                commit_native_mixed_properties(kernel, &property_values, slots);
                string_state.commit();
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.post_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        }
    }
}

#[cfg(test)]
mod native_mixed_array_reserve_tests {
    use super::{
        NATIVE_ARRAY_RESERVE_ENTRY_BUDGET, NATIVE_ARRAY_RESERVE_MIN_ITERATIONS,
        native_mixed_array_reserve_hint,
    };

    #[test]
    fn capacity_hint_shares_one_bounded_budget() {
        assert_eq!(native_mixed_array_reserve_hint(100, 0), 0);
        assert_eq!(native_mixed_array_reserve_hint(100, 1), 0);
        assert_eq!(
            native_mixed_array_reserve_hint(NATIVE_ARRAY_RESERVE_MIN_ITERATIONS, 1),
            NATIVE_ARRAY_RESERVE_MIN_ITERATIONS as usize
        );
        assert_eq!(
            native_mixed_array_reserve_hint(u64::MAX, 1),
            NATIVE_ARRAY_RESERVE_ENTRY_BUDGET
        );
        assert_eq!(
            native_mixed_array_reserve_hint(u64::MAX, 4),
            NATIVE_ARRAY_RESERVE_ENTRY_BUDGET / 4
        );
        assert_eq!(
            native_mixed_array_reserve_hint(u64::MAX, 16),
            NATIVE_ARRAY_RESERVE_ENTRY_BUDGET / 16
        );
    }
}
