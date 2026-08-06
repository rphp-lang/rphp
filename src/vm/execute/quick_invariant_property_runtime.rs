// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_invariant_property_accumulate_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongInvariantPropertyAccumulateKernel,
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

    if !continue_loop {
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
        stats::inc_quick_loop_completed(0);
        return Ok(QuickLoopOutcome::Completed);
    }

    dirty_long_mask |= kernel.property_output_mask;
    let term = if let Some(rhs) = kernel.term_rhs {
        let Some(term) = slots[kernel.term_lhs as usize]
            .checked_add(slots[rhs as usize])
        else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.term_resume_ip,
                iterations,
            ));
        };
        let result = kernel.term_result.unwrap_unchecked();
        slots[result as usize] = term;
        dirty_long_mask |= 1u64 << result;
        term
    } else {
        slots[kernel.term_lhs as usize]
    };

    while continue_loop {
        let Some(sum) = slots[kernel.accumulator as usize].checked_add(term) else {
            return Ok(deopt_quick_long_kernel(
                frame,
                op_array,
                slot_base,
                &slots,
                dirty_long_mask,
                dirty_bool_mask,
                kernel.sum_resume_ip,
                iterations,
            ));
        };
        slots[kernel.sum_result as usize] = sum;
        slots[kernel.accumulator as usize] = sum;
        dirty_long_mask |=
            (1u64 << kernel.sum_result) | (1u64 << kernel.accumulator);

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
            let next_target = if continue_loop {
                kernel.body_target
            } else {
                kernel.exit_target
            };
            let next_ip = plan.target_ip(next_target).unwrap_unchecked();
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
