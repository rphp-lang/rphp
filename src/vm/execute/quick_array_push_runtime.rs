// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// Derive a bounded append count only when the loop's signed bound cannot be
/// overwritten by its induction, condition or post-increment result slots.
#[cfg(feature = "quick-loops")]
#[inline(always)]
fn quick_packed_array_reserve_hint(
    slots: &[i64; 64],
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    condition_tmp: Option<u16>,
    post_value: u16,
    post_result: Option<u16>,
) -> usize {
    if header_lhs != post_value
        || condition_tmp == Some(post_value)
        || post_result == Some(post_value)
        || condition_tmp.is_some() && condition_tmp == post_result
    {
        return 0;
    }
    if let QuickLongOperand::Slot(bound_slot) = header_rhs
        && (bound_slot == post_value
            || condition_tmp == Some(bound_slot)
            || post_result == Some(bound_slot))
    {
        return 0;
    }
    let current = slots[post_value as usize];
    let bound = quick_long_operand(slots, header_rhs);
    if current >= bound {
        return 0;
    }
    let remaining = current.abs_diff(bound);
    if remaining < QUICK_PACKED_ARRAY_RESERVE_MIN_ITERATIONS {
        return 0;
    }
    usize::try_from(remaining)
        .unwrap_or(usize::MAX)
        .min(QUICK_PACKED_ARRAY_RESERVE_ENTRY_BUDGET)
}

/// Keep the one-time allocation path out of the shared hot executor layout.
/// The quick array kernel pays one outlined call before entering its loop.
#[cfg(feature = "quick-loops")]
#[cold]
#[inline(never)]
#[cfg_attr(
    target_os = "linux",
    unsafe(link_section = ".rphp_packed_array_reserve")
)]
fn reserve_quick_packed_array_loop_capacity(
    array: &mut PhpArray,
    slots: &[i64; 64],
    kernel: QuickArrayPushLoopKernel,
) {
    let reserve_hint = quick_packed_array_reserve_hint(
        slots,
        kernel.header_lhs,
        kernel.header_rhs,
        kernel.header_condition_tmp,
        kernel.post_value,
        kernel.post_result,
    );
    if reserve_hint != 0 {
        let reserved = array.reserve_packed_long_appends(reserve_hint);
        stats::record_quick_packed_array_reserve(reserve_hint, reserved);
    }
}

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
    reserve_quick_packed_array_loop_capacity(&mut *array, &slots, kernel);
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

#[cfg(all(test, feature = "quick-loops"))]
mod quick_packed_array_reserve_tests {
    use super::{
        QUICK_PACKED_ARRAY_RESERVE_ENTRY_BUDGET, QUICK_PACKED_ARRAY_RESERVE_MIN_ITERATIONS,
        QuickLongOperand, quick_packed_array_reserve_hint,
    };

    #[test]
    fn hint_is_bounded_and_requires_a_stable_large_unit_loop() {
        let mut slots = [0; 64];
        slots[1] = QUICK_PACKED_ARRAY_RESERVE_MIN_ITERATIONS as i64;
        assert_eq!(
            quick_packed_array_reserve_hint(
                &slots,
                0,
                QuickLongOperand::Slot(1),
                Some(2),
                0,
                Some(3),
            ),
            QUICK_PACKED_ARRAY_RESERVE_MIN_ITERATIONS as usize
        );

        slots[1] = i64::MAX;
        assert_eq!(
            quick_packed_array_reserve_hint(
                &slots,
                0,
                QuickLongOperand::Slot(1),
                Some(2),
                0,
                Some(3),
            ),
            QUICK_PACKED_ARRAY_RESERVE_ENTRY_BUDGET
        );

        slots[1] = QUICK_PACKED_ARRAY_RESERVE_MIN_ITERATIONS as i64 - 1;
        assert_eq!(
            quick_packed_array_reserve_hint(
                &slots,
                0,
                QuickLongOperand::Slot(1),
                Some(2),
                0,
                Some(3),
            ),
            0
        );
        assert_eq!(
            quick_packed_array_reserve_hint(
                &slots,
                0,
                QuickLongOperand::Slot(1),
                Some(1),
                0,
                Some(3),
            ),
            0
        );
    }
}
