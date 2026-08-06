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

unsafe fn prepare_native_mixed_properties(
    kernel: &NativeQuickLongMixedKernel,
    resolved_object_ops: &[QuickResolvedObjectOp],
    slots: &mut [i64; 64],
) -> Option<[*mut Value; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES]> {
    let mut values = [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    for index in 0..kernel.property_binding_count as usize {
        let op_index = kernel.property_binding_op_indices[index] as usize;
        let property_index = kernel.property_binding_property_indices[index] as usize;
        let QuickResolvedObjectOp::PropertyMethod {
            receiver,
            property_slots,
            property_count,
            ..
        } = *resolved_object_ops.get(op_index)?
        else {
            return None;
        };
        if receiver.is_null() || property_index >= property_count as usize {
            return None;
        }
        let value = (*receiver)
            .object_property_slot_unchecked(property_slots[property_index])
            as *mut Value;
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
    for (index, operation) in plan.ops.iter().copied().enumerate() {
        let (result, string_length) = match operation {
            QuickLongOp::ObjectPropertyLong { result, .. } => (result, false),
            QuickLongOp::ObjectPropertyStringLength { result, .. } => (result, true),
            _ => continue,
        };
        let Some(QuickResolvedObjectOp::PropertyRead { property }) =
            resolved_object_ops.get(index).copied()
        else {
            return Ok(None);
        };
        if property.is_null() || (*property).is_reference() {
            return Ok(None);
        }
        slots[result as usize] = if string_length {
            let Some(value) = (*property).as_str() else {
                return Ok(None);
            };
            value.len() as i64
        } else {
            if (*property).value_type() != ValueType::Long {
                return Ok(None);
            }
            (*property).raw_long()
        };
    }

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

    let mut entry_pointers =
        [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    for index in 0..kernel.context_count as usize {
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
        entry_pointers[index] = value as *mut Value as *mut i64;
    }

    let cache = plan.native_jit();
    let Some(program) = cache.prepare_straight_program(&kernel.config) else {
        return Ok(None);
    };
    let bound = quick_long_operand(slots, kernel.config.bound);
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
            before_entries[index] = *entry_pointers[index];
        }

        let native_result = cache.dispatch_prepared_straight_chunk_with_context(
            program,
            slots,
            NATIVE_LONG_SAFEPOINT_INTERVAL,
            &entry_pointers,
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
                    *entry_pointers[index] = before_entries[index];
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
