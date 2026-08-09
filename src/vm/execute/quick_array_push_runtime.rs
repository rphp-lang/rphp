// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// Run a closed loop whose only body operation appends one Long value. The
/// array pointer comes from the canonical unique-COW entry guard, and each
/// push remains immediately observable before a possible increment fallback.
#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_array_push_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    array: *mut PhpArray,
    kernel: QuickArrayPushLoopKernel,
) -> Result<QuickLoopOutcome, VmError> {
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;
    let mut continue_loop =
        slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
    if let Some(slot) = kernel.header_condition_tmp {
        slots[slot as usize] = i64::from(continue_loop);
        dirty_bool_mask |= 1u64 << slot;
    }

    while continue_loop {
        if let Some((mut current, end, bound)) = quick_unit_loop_chunk(
            &slots,
            kernel.header_lhs,
            kernel.header_rhs,
            kernel.header_condition_tmp,
            kernel.post_value,
            kernel.post_result,
        ) {
            let mut values = [0i64; QUICK_UNIT_LOOP_CHUNK as usize];
            for value in values.iter_mut() {
                *value = quick_long_operand(&slots, kernel.value);
                if let Some(result) = kernel.post_result {
                    slots[result as usize] = current;
                }
                current += 1;
                slots[kernel.post_value as usize] = current;
            }
            if !(*array).push_packed_long_chunk(&values) {
                for value in values {
                    (*array).push(Value::long(value));
                }
            }
            debug_assert_eq!(current, end);
            if let Some(result) = kernel.post_result {
                dirty_long_mask |= 1u64 << result;
            }
            dirty_long_mask |= 1u64 << kernel.post_value;
            continue_loop = end < bound;
            if let Some(slot) = kernel.header_condition_tmp {
                slots[slot as usize] = i64::from(continue_loop);
                dirty_bool_mask |= 1u64 << slot;
            }
            iterations += QUICK_UNIT_LOOP_CHUNK as u64;

            if eg.vm_interrupt.load(Ordering::Relaxed) {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                let target = if continue_loop {
                    kernel.body_target
                } else {
                    kernel.exit_target
                };
                let next_ip = plan.target_ip(target).unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                handle_interrupt(eg)?;
            }
            continue;
        }

        let value = quick_long_operand(&slots, kernel.value);
        (*array).push(Value::long(value));

        let Some(incremented) = slots[kernel.post_value as usize].checked_add(1) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.post_resume_ip,
                iterations,
            ));
        };
        if let Some(result) = kernel.post_result {
            slots[result as usize] = slots[kernel.post_value as usize];
            dirty_long_mask |= 1u64 << result;
        }
        slots[kernel.post_value as usize] = incremented;
        dirty_long_mask |= 1u64 << kernel.post_value;

        continue_loop =
            slots[kernel.header_lhs as usize] < quick_long_operand(&slots, kernel.header_rhs);
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(continue_loop);
            dirty_bool_mask |= 1u64 << slot;
        }
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            commit_quick_long_ops_slots(
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
            );
            let target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(target).unwrap_unchecked();
            (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
            handle_interrupt(eg)?;
        }
    }

    commit_quick_long_ops_slots(
        slot_base,
        &slots,
        dirty_long_mask,
        dirty_bool_mask,
    );
    (*frame).opline = op_array
        .instructions
        .as_ptr()
        .add(kernel.exit_target.exit_ip().unwrap_unchecked());
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}
