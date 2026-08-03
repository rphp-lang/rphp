// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_induction_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: QuickLongInductionLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs
        || (*frame).num_cvs + (*frame).num_temps > 64
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let induction_ptr = slot_base.add(plan.induction_cv as usize);
    let condition_ptr = plan
        .condition_tmp
        .map(|slot| slot_base.add(slot as usize));
    let increment_ptr = plan
        .increment_tmp
        .map(|slot| slot_base.add(slot as usize));
    let bound_ptr = match plan.bound {
        QuickLongBound::Cv(slot) => Some(slot_base.add(slot as usize)),
        QuickLongBound::Const(_) => None,
    };

    if quick_loop_slot_has_heap(frame, plan.induction_cv)
        || plan
            .condition_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || plan
            .increment_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || matches!(plan.bound, QuickLongBound::Cv(slot) if quick_loop_slot_has_heap(frame, slot))
        || (*induction_ptr).value_type() != ValueType::Long
        || condition_ptr.is_some_and(|ptr| {
            !matches!((*ptr).value_type(), ValueType::True | ValueType::False)
        })
        || increment_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || bound_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let mut induction = (*induction_ptr).raw_long();
    let bound = match plan.bound {
        QuickLongBound::Cv(_) => (*bound_ptr.unwrap_unchecked()).raw_long(),
        QuickLongBound::Const(value) => value,
    };
    let mut iterations = 0u64;
    let mut last_increment_result = 0i64;
    let mut completed_iteration = false;

    loop {
        if induction >= bound {
            Value::write_long(induction_ptr, induction);
            if let Some(ptr) = condition_ptr {
                Value::write_bool(ptr, false);
            }
            if completed_iteration {
                if let Some(ptr) = increment_ptr {
                    Value::write_long(ptr, last_increment_result);
                }
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
            stats::inc_quick_loop_completed(iterations);
            return Ok(QuickLoopOutcome::Completed);
        }

        let next_induction = match induction.checked_add(1) {
            Some(value) => value,
            None => {
                Value::write_long(induction_ptr, induction);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.increment_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };

        last_increment_result = match plan.increment_kind {
            QuickIncrementKind::Pre => next_induction,
            QuickIncrementKind::Post => induction,
        };
        induction = next_induction;
        completed_iteration = true;
        iterations += 1;

        // The baseline region has four instructions, so checking every 64
        // iterations preserves execute_ex's 256-opcode interrupt interval.
        if iterations & 63 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            Value::write_long(induction_ptr, induction);
            if let Some(ptr) = condition_ptr {
                Value::write_bool(ptr, true);
            }
            if let Some(ptr) = increment_ptr {
                Value::write_long(ptr, last_increment_result);
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
            handle_interrupt(eg)?;
        }
    }
}

