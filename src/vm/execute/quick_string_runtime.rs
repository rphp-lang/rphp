// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// Run a closed loop whose only body operation appends one invariant String.
/// The planner already proved the destination unique and the source immutable;
/// this removes three-way typed-op dispatch while retaining the same checked
/// increment, frame publication and interrupt points as the general executor.
#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_string_append_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    destination: *mut String,
    kernel: QuickStringAppendLoopKernel,
) -> Result<QuickLoopOutcome, VmError> {
    let source = match kernel.source {
        QuickStringAppendSource::Literal(literal) => op_array
            .literals
            .get_unchecked(literal as usize)
            .as_str()
            .unwrap_unchecked(),
        QuickStringAppendSource::Slot(slot) => {
            (*slot_base.add(slot as usize)).as_str().unwrap_unchecked()
        }
    };
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
        (*destination).push_str(source);

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
