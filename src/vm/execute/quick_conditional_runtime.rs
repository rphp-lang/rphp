// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_conditional_kernel<F>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongConditionalKernel,
    mut evaluate_body_condition: F,
) -> Result<QuickLoopOutcome, VmError>
where
    F: FnMut(&mut [i64; 64], &mut u64, &mut u64) -> Result<bool, usize>,
{
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
        let body_condition = match evaluate_body_condition(
            &mut slots,
            &mut dirty_long_mask,
            &mut dirty_bool_mask,
        ) {
            Ok(condition) => condition,
            Err(resume_ip) => {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };

        if body_condition {
            let value = match slots[kernel.add_lhs as usize]
                .checked_add(slots[kernel.add_rhs as usize])
            {
                Some(value) => value,
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline = op_array
                        .instructions
                        .as_ptr()
                        .add(kernel.add_resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            };
            slots[kernel.add_result as usize] = value;
            slots[kernel.destination as usize] = value;
            dirty_long_mask |=
                (1u64 << kernel.add_result) | (1u64 << kernel.destination);
        }

        let incremented = match slots[kernel.post_value as usize].checked_add(1) {
            Some(value) => value,
            None => {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.post_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
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
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_branch_only_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    mut slots: [i64; 64],
    kernel: QuickLongBranchOnlyKernel,
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
        for condition in &kernel.conditions[..kernel.condition_count as usize] {
            let condition_result = slots[condition.lhs as usize]
                == quick_long_operand(&slots, condition.rhs);
            if let Some(slot) = condition.condition_tmp {
                slots[slot as usize] = i64::from(condition_result);
                dirty_bool_mask |= 1u64 << slot;
            }
            if condition_result {
                break;
            }
        }

        let incremented = match slots[kernel.post_value as usize].checked_add(1) {
            Some(value) => value,
            None => {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.post_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
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
    let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
fn native_conditional_long_loop_config(
    kernel: QuickLongConditionalKernel,
    body: QuickLongConditionalBody,
) -> Option<NativeConditionalLongLoopConfig> {
    let induction = kernel.post_value;
    if kernel.header_lhs != induction {
        return None;
    }
    let accumulator = kernel.destination;
    let adds_induction_to_accumulator = (kernel.add_lhs == accumulator
        && kernel.add_rhs == induction)
        || (kernel.add_rhs == accumulator && kernel.add_lhs == induction);
    if !adds_induction_to_accumulator
        || accumulator == induction
        || kernel.add_result == induction
        || kernel.post_result.is_some_and(|slot| {
            slot == induction || slot == accumulator
        })
    {
        return None;
    }

    let condition = match body {
        QuickLongConditionalBody::LessThan { lhs, rhs, .. } if lhs == induction => {
            NativeConditionalLongLoopCondition::LessThan { rhs }
        }
        QuickLongConditionalBody::ModuloEqual {
            value,
            divisor,
            result,
            lhs,
            rhs,
            ..
        } if value == induction
            && lhs == result
            && result != induction
            && result != accumulator
            && result != kernel.add_result
            && kernel.post_result != Some(result) =>
        {
            NativeConditionalLongLoopCondition::ModuloEqual { divisor, rhs }
        }
        _ => return None,
    };
    let modulo_result = match body {
        QuickLongConditionalBody::ModuloEqual { result, .. } => Some(result),
        QuickLongConditionalBody::LessThan { .. } => None,
    };
    let condition_rhs = match condition {
        NativeConditionalLongLoopCondition::LessThan { rhs }
        | NativeConditionalLongLoopCondition::ModuloEqual { rhs, .. } => rhs,
    };
    let mut mutable_long_mask =
        (1u64 << induction) | (1u64 << accumulator) | (1u64 << kernel.add_result);
    if let Some(slot) = kernel.post_result {
        mutable_long_mask |= 1u64 << slot;
    }
    if let Some(slot) = modulo_result {
        mutable_long_mask |= 1u64 << slot;
    }
    for operand in [kernel.header_rhs, condition_rhs] {
        if matches!(operand, QuickLongOperand::Slot(slot) if mutable_long_mask & (1u64 << slot) != 0) {
            return None;
        }
    }
    Some(NativeConditionalLongLoopConfig {
        induction_slot: induction,
        bound: kernel.header_rhs,
        condition,
        accumulator_slot: accumulator,
    })
}

#[inline]
fn checked_php_long_modulo(value: i64, divisor: i64) -> Option<i64> {
    if divisor == -1 {
        // PHP defines PHP_INT_MIN % -1 as zero even though Rust's checked_rem
        // reports the corresponding signed-division overflow.
        Some(0)
    } else {
        value.checked_rem(divisor)
    }
}

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
fn publish_native_conditional_body_state(
    slots: &mut [i64; 64],
    body: QuickLongConditionalBody,
    induction: i64,
    dirty_long_mask: &mut u64,
    dirty_bool_mask: &mut u64,
) -> bool {
    match body {
        QuickLongConditionalBody::LessThan {
            rhs,
            condition_tmp,
            ..
        } => {
            let condition = induction < quick_long_operand(slots, rhs);
            if let Some(slot) = condition_tmp {
                slots[slot as usize] = i64::from(condition);
                *dirty_bool_mask |= 1u64 << slot;
            }
            true
        }
        QuickLongConditionalBody::ModuloEqual {
            divisor,
            result,
            rhs,
            condition_tmp,
            ..
        } => {
            let Some(remainder) = checked_php_long_modulo(induction, divisor) else {
                return false;
            };
            slots[result as usize] = remainder;
            *dirty_long_mask |= 1u64 << result;
            let condition = remainder == quick_long_operand(slots, rhs);
            if let Some(slot) = condition_tmp {
                slots[slot as usize] = i64::from(condition);
                *dirty_bool_mask |= 1u64 << slot;
            }
            true
        }
    }
}

#[inline(never)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
unsafe fn run_native_quick_long_conditional_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: &mut [i64; 64],
    kernel: QuickLongConditionalKernel,
    body: QuickLongConditionalBody,
) -> Result<Option<QuickLoopOutcome>, VmError> {
    let Some(config) = native_conditional_long_loop_config(kernel, body) else {
        return Ok(None);
    };
    let bound = quick_long_operand(slots, config.bound);
    let cache = plan.native_jit();
    let mut iterations = 0u64;
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut addition_completed = false;
    let mut entered_native = false;

    loop {
        let before_induction = slots[config.induction_slot as usize];
        let before_accumulator = slots[config.accumulator_slot as usize];
        let native_result = cache.dispatch_chunk(
            config,
            slots,
            NATIVE_LONG_ACCUMULATE_CHUNK,
        );
        let (mut outcome, chunk_addition_executed) = match native_result {
            Ok(result) => {
                if !entered_native {
                    cache.record_region_entry();
                    entered_native = true;
                }
                (result.outcome, result.addition_executed)
            }
            Err(_) if !cache.is_conditional_compiled() => return Ok(None),
            Err(_) => {
                slots[config.induction_slot as usize] = before_induction;
                slots[config.accumulator_slot as usize] = before_accumulator;
                if let Some(slot) = kernel.header_condition_tmp {
                    slots[slot as usize] = 1;
                    dirty_bool_mask |= 1u64 << slot;
                }
                if addition_completed {
                    slots[kernel.add_result as usize] =
                        slots[config.accumulator_slot as usize];
                    dirty_long_mask |=
                        (1u64 << kernel.add_result) | (1u64 << kernel.destination);
                }
                if iterations != 0 {
                    if let Some(slot) = kernel.post_result {
                        slots[slot as usize] = before_induction - 1;
                        dirty_long_mask |= 1u64 << slot;
                    }
                    let published = publish_native_conditional_body_state(
                        slots,
                        body,
                        before_induction - 1,
                        &mut dirty_long_mask,
                        &mut dirty_bool_mask,
                    );
                    debug_assert!(published);
                }
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                let next_ip = plan.target_ip(kernel.body_target).unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        };

        let induction = slots[config.induction_slot as usize];
        let completed_in_chunk =
            (induction as u64).wrapping_sub(before_induction as u64);
        iterations = iterations.saturating_add(completed_in_chunk);
        if completed_in_chunk != 0 {
            dirty_long_mask |= 1u64 << config.induction_slot;
            if let Some(slot) = kernel.post_result {
                slots[slot as usize] = induction - 1;
                dirty_long_mask |= 1u64 << slot;
            }
        }
        if chunk_addition_executed {
            addition_completed = true;
        }
        if addition_completed {
            slots[kernel.add_result as usize] =
                slots[config.accumulator_slot as usize];
            dirty_long_mask |=
                (1u64 << kernel.add_result) | (1u64 << kernel.destination);
        }

        if outcome == QuickLongAccumulateJitOutcome::ChunkExhausted && induction >= bound {
            outcome = QuickLongAccumulateJitOutcome::Completed;
        }
        let completed = outcome == QuickLongAccumulateJitOutcome::Completed;
        if let Some(slot) = kernel.header_condition_tmp {
            slots[slot as usize] = i64::from(!completed);
            dirty_bool_mask |= 1u64 << slot;
        }

        let body_induction = match outcome {
            QuickLongAccumulateJitOutcome::SumOverflow
            | QuickLongAccumulateJitOutcome::IncrementOverflow => Some(induction),
            QuickLongAccumulateJitOutcome::ConditionSideExit if iterations != 0 => {
                Some(induction - 1)
            }
            QuickLongAccumulateJitOutcome::Completed
            | QuickLongAccumulateJitOutcome::ChunkExhausted
                if iterations != 0 =>
            {
                Some(induction - 1)
            }
            _ => None,
        };
        if let Some(body_induction) = body_induction {
            let published = publish_native_conditional_body_state(
                slots,
                body,
                body_induction,
                &mut dirty_long_mask,
                &mut dirty_bool_mask,
            );
            debug_assert!(published);
        }

        match outcome {
            QuickLongAccumulateJitOutcome::Completed => {
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                let next_ip = kernel.exit_target.exit_ip().unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                stats::inc_quick_loop_completed(iterations);
                return Ok(Some(QuickLoopOutcome::Completed));
            }
            QuickLongAccumulateJitOutcome::ChunkExhausted => {
                debug_assert_eq!(completed_in_chunk, NATIVE_LONG_ACCUMULATE_CHUNK);
                if eg.vm_interrupt.load(Ordering::Relaxed) {
                    commit_quick_long_ops_slots(
                        slot_base,
                        slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    let next_ip = plan.target_ip(kernel.body_target).unwrap_unchecked();
                    (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                    handle_interrupt(eg)?;
                }
            }
            QuickLongAccumulateJitOutcome::SumOverflow => {
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.add_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            QuickLongAccumulateJitOutcome::IncrementOverflow => {
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(kernel.post_resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            QuickLongAccumulateJitOutcome::ConditionSideExit => {
                let resume_ip = match body {
                    QuickLongConditionalBody::ModuloEqual { resume_ip, .. } => resume_ip,
                    QuickLongConditionalBody::LessThan { .. } => {
                        unreachable!("less-than lowering has no condition side exit")
                    }
                };
                commit_quick_long_ops_slots(
                    slot_base,
                    slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            QuickLongAccumulateJitOutcome::TermOverflow => {
                unreachable!("conditional Long IR does not compute a separate term")
            }
        }
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn dispatch_quick_long_conditional_kernel(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: [i64; 64],
    kernel: QuickLongConditionalKernel,
    body: QuickLongConditionalBody,
) -> Result<QuickLoopOutcome, VmError> {
    #[cfg(all(
        feature = "jit-prototype",
        target_arch = "aarch64",
        target_os = "macos"
    ))]
    {
        let mut native_slots = slots;
        if let Some(outcome) = run_native_quick_long_conditional_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            &mut native_slots,
            kernel,
            body,
        )? {
            return Ok(outcome);
        }
    }

    match body {
        QuickLongConditionalBody::LessThan {
            lhs,
            rhs,
            condition_tmp,
        } => run_quick_long_conditional_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            move |slots, _, dirty_bool_mask| {
                let condition = slots[lhs as usize] < quick_long_operand(slots, rhs);
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    *dirty_bool_mask |= 1u64 << slot;
                }
                Ok(condition)
            },
        ),
        QuickLongConditionalBody::ModuloEqual {
            value,
            divisor,
            result,
            resume_ip,
            lhs,
            rhs,
            condition_tmp,
        } => run_quick_long_conditional_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
            move |slots, dirty_long_mask, dirty_bool_mask| {
                let remainder =
                    checked_php_long_modulo(slots[value as usize], divisor).ok_or(resume_ip)?;
                slots[result as usize] = remainder;
                *dirty_long_mask |= 1u64 << result;
                let condition = slots[lhs as usize] == quick_long_operand(slots, rhs);
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    *dirty_bool_mask |= 1u64 << slot;
                }
                Ok(condition)
            },
        ),
    }
}
