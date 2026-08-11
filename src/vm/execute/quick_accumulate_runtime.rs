// Kept in the execute module through include! so this structural split does not change visibility or code generation.

const QUICK_INDUCTION_CONST_CHUNK: i64 = 32;
const QUICK_INDUCTION_CONST_TRIANGLE: i64 =
    (QUICK_INDUCTION_CONST_CHUNK - 1) * QUICK_INDUCTION_CONST_CHUNK / 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QuickInductionConstChunk {
    induction: i64,
    accumulator: i64,
    last_term: i64,
    last_increment_result: i64,
}

/// Fold one interrupt-sized arithmetic progression. Returning `None` keeps
/// the canonical single-iteration path, which preserves the exact PHP
/// overflow position for edge ranges and sign-crossing progressions.
#[inline(always)]
fn quick_induction_const_chunk(
    induction: i64,
    accumulator: i64,
    bound: i64,
    addend: i64,
    increment_kind: QuickIncrementKind,
) -> Option<QuickInductionConstChunk> {
    let next_induction = induction.checked_add(QUICK_INDUCTION_CONST_CHUNK)?;
    if next_induction > bound {
        return None;
    }
    let first_term = induction.checked_add(addend)?;
    let last_term = first_term.checked_add(QUICK_INDUCTION_CONST_CHUNK - 1)?;
    if first_term < 0 && last_term > 0 {
        return None;
    }
    let term_sum = first_term
        .checked_mul(QUICK_INDUCTION_CONST_CHUNK)?
        .checked_add(QUICK_INDUCTION_CONST_TRIANGLE)?;
    let accumulator = accumulator.checked_add(term_sum)?;
    let last_increment_result = match increment_kind {
        QuickIncrementKind::Pre => next_induction,
        QuickIncrementKind::Post => next_induction - 1,
    };
    Some(QuickInductionConstChunk {
        induction: next_induction,
        accumulator,
        last_term,
        last_increment_result,
    })
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongAccumulateLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs
        || (*frame).num_cvs + (*frame).num_temps > 64
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let induction_ptr = slot_base.add(plan.induction_cv as usize);
    let accumulator_ptr = slot_base.add(plan.accumulator_cv as usize);
    let condition_ptr = plan
        .condition_tmp
        .map(|slot| slot_base.add(slot as usize));
    let tail_condition_ptr = plan
        .tail_guard
        .and_then(|guard| guard.condition_tmp)
        .map(|slot| slot_base.add(slot as usize));
    let term_ptr = match plan.term {
        QuickLongTerm::Induction => None,
        QuickLongTerm::InductionPlusConst { term_tmp, .. }
        | QuickLongTerm::InductionPlusCv { term_tmp, .. }
        | QuickLongTerm::ArrayIndex { term_tmp, .. }
        | QuickLongTerm::StringLength { term_tmp, .. }
        | QuickLongTerm::AbsLong { term_tmp, .. }
        | QuickLongTerm::ScalarFunctionCall { term_tmp, .. }
        | QuickLongTerm::ScalarCallTree { term_tmp, .. } => {
            Some(slot_base.add(term_tmp as usize))
        }
    };
    let term_destination_ptr = match plan.term {
        QuickLongTerm::ArrayIndex {
            destination: Some(destination),
            ..
        } => Some(slot_base.add(destination as usize)),
        _ => None,
    };
    let addend_ptr = match plan.term {
        QuickLongTerm::Induction
        | QuickLongTerm::InductionPlusConst { .. }
        | QuickLongTerm::ArrayIndex { .. }
        | QuickLongTerm::StringLength { .. }
        | QuickLongTerm::AbsLong { .. }
        | QuickLongTerm::ScalarFunctionCall { .. }
        | QuickLongTerm::ScalarCallTree { .. } => None,
        QuickLongTerm::InductionPlusCv { addend_cv, .. } => {
            Some(slot_base.add(addend_cv as usize))
        }
    };
    let array_ptr = match plan.term {
        QuickLongTerm::ArrayIndex { array_cv, .. } => {
            Some(slot_base.add(array_cv as usize))
        }
        _ => None,
    };
    let string_ptr = match plan.term {
        QuickLongTerm::StringLength { string_cv, .. } => {
            Some(slot_base.add(string_cv as usize))
        }
        _ => None,
    };
    let abs_operand_ptr = match plan.term {
        QuickLongTerm::AbsLong { operand_cv, .. } => {
            Some(slot_base.add(operand_cv as usize))
        }
        _ => None,
    };
    let sum_ptr = slot_base.add(plan.sum_tmp as usize);
    let increment_ptr = plan
        .increment_tmp
        .map(|slot| slot_base.add(slot as usize));

    let bound_ptr = match plan.bound {
        QuickLongBound::Cv(slot) => Some(slot_base.add(slot as usize)),
        QuickLongBound::Const(_) => None,
    };

    let scalar_call_inputs_valid = match plan.term {
        QuickLongTerm::ScalarFunctionCall {
            mut long_input_mask,
            ..
        } => {
            let mut valid = true;
            while long_input_mask != 0 {
                let slot = long_input_mask.trailing_zeros() as u16;
                long_input_mask &= long_input_mask - 1;
                valid &= !quick_loop_slot_has_heap(frame, slot)
                    && (*slot_base.add(slot as usize)).value_type() == ValueType::Long;
            }
            valid
        }
        QuickLongTerm::ScalarCallTree {
            mut long_input_mask,
            mut object_input_mask,
            ..
        } => {
            let mut valid = true;
            while long_input_mask != 0 {
                let slot = long_input_mask.trailing_zeros() as u16;
                long_input_mask &= long_input_mask - 1;
                valid &= !quick_loop_slot_has_heap(frame, slot)
                    && (*slot_base.add(slot as usize)).value_type() == ValueType::Long;
            }
            while object_input_mask != 0 {
                let slot = object_input_mask.trailing_zeros() as u16;
                object_input_mask &= object_input_mask - 1;
                let value = &*slot_base.add(slot as usize);
                valid &= value.value_type() == ValueType::Object
                    && !value.is_reference()
                    && value.object_class_id_unchecked() != 0;
            }
            valid
        }
        _ => true,
    };
    let tail_guard_inputs_valid = plan.tail_guard.is_none_or(|guard| {
        [guard.lhs, guard.rhs].into_iter().all(|operand| match operand {
            QuickLongOperand::Const(_) => true,
            QuickLongOperand::Slot(slot) => {
                !quick_loop_slot_has_heap(frame, slot)
                    && (*slot_base.add(slot as usize)).value_type() == ValueType::Long
            }
        })
    });

    if quick_loop_slot_has_heap(frame, plan.induction_cv)
        || quick_loop_slot_has_heap(frame, plan.accumulator_cv)
        || plan
            .condition_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || matches!(
            plan.term,
            QuickLongTerm::InductionPlusConst { term_tmp, .. }
                | QuickLongTerm::InductionPlusCv { term_tmp, .. }
                | QuickLongTerm::ArrayIndex { term_tmp, .. }
                | QuickLongTerm::StringLength { term_tmp, .. }
                | QuickLongTerm::AbsLong { term_tmp, .. }
                | QuickLongTerm::ScalarFunctionCall { term_tmp, .. }
                | QuickLongTerm::ScalarCallTree { term_tmp, .. }
                if quick_loop_slot_has_heap(frame, term_tmp)
        )
        || matches!(
            plan.term,
            QuickLongTerm::InductionPlusCv { addend_cv, .. }
                if quick_loop_slot_has_heap(frame, addend_cv)
        )
        || matches!(
            plan.term,
            QuickLongTerm::AbsLong { operand_cv, .. }
                if quick_loop_slot_has_heap(frame, operand_cv)
        )
        || matches!(
            plan.term,
            QuickLongTerm::ArrayIndex {
                destination: Some(destination),
                ..
            } if quick_loop_slot_has_heap(frame, destination)
        )
        || quick_loop_slot_has_heap(frame, plan.sum_tmp)
        || plan
            .increment_tmp
            .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        || plan.tail_guard.is_some_and(|guard| {
            guard
                .condition_tmp
                .is_some_and(|slot| quick_loop_slot_has_heap(frame, slot))
        })
        || matches!(plan.bound, QuickLongBound::Cv(slot) if quick_loop_slot_has_heap(frame, slot))
        || !scalar_call_inputs_valid
        || !tail_guard_inputs_valid
        || (*induction_ptr).value_type() != ValueType::Long
        || (*accumulator_ptr).value_type() != ValueType::Long
        || condition_ptr.is_some_and(|ptr| {
            !matches!((*ptr).value_type(), ValueType::True | ValueType::False)
        })
        || tail_condition_ptr.is_some_and(|ptr| {
            !matches!((*ptr).value_type(), ValueType::True | ValueType::False)
        })
        || term_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || term_destination_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || addend_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || array_ptr.is_some_and(|ptr| (*ptr).as_array().is_none())
        || string_ptr.is_some_and(|ptr| (*ptr).as_str().is_none())
        || abs_operand_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || (*sum_ptr).value_type() != ValueType::Long
        || increment_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
        || bound_ptr.is_some_and(|ptr| (*ptr).value_type() != ValueType::Long)
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let mut induction = (*induction_ptr).raw_long();
    let mut accumulator = (*accumulator_ptr).raw_long();
    let bound = match plan.bound {
        QuickLongBound::Cv(_) => (*bound_ptr.unwrap_unchecked()).raw_long(),
        QuickLongBound::Const(value) => value,
    };
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    if plan.tail_guard.is_some()
        && let Some(outcome) = run_native_guarded_long_accumulate_loop(
            eg,
            frame,
            op_array,
            plan,
            induction_ptr,
            accumulator_ptr,
            condition_ptr,
            tail_condition_ptr,
            term_ptr,
            sum_ptr,
            increment_ptr,
            induction,
            accumulator,
            bound,
        )?
    {
        #[cfg(feature = "vm-stats")]
        record_native_quick_outcome(
            stats::JitRegionKind::LongAccumulate,
            &outcome,
        );
        return Ok(outcome);
    }
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    if let Some(outcome) = run_native_long_accumulate_loop(
        eg,
        frame,
        op_array,
        plan,
        induction_ptr,
        accumulator_ptr,
        condition_ptr,
        term_ptr,
        sum_ptr,
        increment_ptr,
        induction,
        accumulator,
        bound,
    )? {
        #[cfg(feature = "vm-stats")]
        record_native_quick_outcome(
            stats::JitRegionKind::LongAccumulate,
            &outcome,
        );
        return Ok(outcome);
    }
    let invariant_addend = addend_ptr.map(|ptr| (*ptr).raw_long());
    let quick_array = array_ptr.map(|ptr| {
        QuickLongArray::from_array((*ptr).as_array().unwrap_unchecked())
    });
    let invariant_string_length = string_ptr.map(|ptr| {
        (*ptr).as_str().unwrap_unchecked().len() as i64
    });
    let invariant_abs = match plan.term {
        QuickLongTerm::AbsLong { operand_cv, .. } if operand_cv != plan.induction_cv => {
            (*abs_operand_ptr.unwrap_unchecked()).raw_long().checked_abs()
        }
        _ => Some(0),
    };
    let invariant_array_term = match plan.term {
        QuickLongTerm::ArrayIndex {
            index: QuickArrayIndex::Long(QuickLongOperand::Const(index)),
            ..
        } => quick_array.unwrap_unchecked().long_at_int(index),
        QuickLongTerm::ArrayIndex {
            index: QuickArrayIndex::StringLiteral(literal),
            ..
        } => {
            let key = op_array
                .literals
                .get_unchecked(literal as usize)
                .as_str()
                .unwrap_unchecked();
            quick_array.unwrap_unchecked().long_at_str(key)
        }
        QuickLongTerm::ArrayIndex {
            index: QuickArrayIndex::ValueSlot(slot),
            ..
        } => match value_to_array_key_ref(&*slot_base.add(slot as usize)).ok() {
            Some(ArrayKeyRef::Int(key)) => quick_array.unwrap_unchecked().long_at_int(key),
            Some(ArrayKeyRef::String(key)) => quick_array.unwrap_unchecked().long_at_str(key),
            None => None,
        },
        _ => Some(0),
    };
    if invariant_array_term.is_none() || invariant_abs.is_none() {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }
    let mut scalar_call_common = std::ptr::null();
    let mut scalar_call_user: *const UserFunction = std::ptr::null();
    let mut scalar_call_plan: *const ScalarLongFunctionPlan = std::ptr::null();
    let mut scalar_call_composed_plan: *const ComposedScalarLongFunctionPlan =
        std::ptr::null();
    let mut scalar_call_typed_plan: *const ComposedTypedLongFunctionPlan =
        std::ptr::null();
    let mut scalar_call_long_argument_mask = 0u8;
    let mut scalar_call_object_arguments = [std::ptr::null(); 8];
    let mut scalar_call_fused_program: Option<ScalarLongProgram<ScalarLongOp, 1>> = None;
    let mut quick_composed_targets =
        [std::ptr::null(); COMPOSED_SCALAR_MAX_OPS];
    let mut quick_composed_plans =
        [std::ptr::null(); COMPOSED_SCALAR_MAX_OPS];
    let mut quick_composed_string_plans =
        [std::ptr::null(); COMPOSED_SCALAR_MAX_OPS];
    let quick_composed_string_arguments = [None; 8];
    let mut quick_composed_leaf_body = false;
    if let QuickLongTerm::ScalarFunctionCall {
        guard,
        argument_count,
        ..
    } = plan.term
    {
        let Some((cached, user)) = guarded_quick_scalar_call_target(
            eg,
            op_array,
            slot_base,
            guard,
            argument_count,
        ) else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        let user = &*user;
        let QuickLongTerm::ScalarFunctionCall { argument_plan, .. } = &plan.term else {
            unreachable!("scalar function call setup")
        };
        if let Some(scalar_plan) = user.scalar_long_plan.as_deref() {
            if scalar_plan.public_args != argument_count
                || !quick_long_argument_outputs_are_valid(
                    slot_base,
                    argument_plan,
                    argument_count,
                )
            {
                stats::inc_quick_loop_guard_failed();
                return Ok(QuickLoopOutcome::GuardFailed);
            }
            scalar_call_plan = scalar_plan;
            scalar_call_long_argument_mask = if argument_count == 8 {
                u8::MAX
            } else {
                (1u8 << argument_count) - 1
            };
            scalar_call_fused_program =
                compose_quick_scalar_leaf_program(argument_plan, scalar_plan);
        } else if composed_scalar_bodies_enabled() {
            let (public_args, long_mask, object_mask) = if let Some(composed_plan) =
                user.composed_scalar_long_plan.as_deref()
            {
                scalar_call_composed_plan = composed_plan;
                (
                    composed_plan.public_args,
                    composed_plan.long_argument_mask,
                    composed_plan.object_argument_mask,
                )
            } else if let Some(typed_plan) = user.composed_typed_long_plan.as_deref()
                && typed_plan.string_argument_mask == 0
            {
                scalar_call_typed_plan = typed_plan;
                (
                    typed_plan.public_args,
                    typed_plan.long_argument_mask,
                    typed_plan.object_argument_mask,
                )
            } else {
                stats::inc_quick_loop_guard_failed();
                return Ok(QuickLoopOutcome::GuardFailed);
            };
            if public_args != argument_count {
                stats::inc_quick_loop_guard_failed();
                return Ok(QuickLoopOutcome::GuardFailed);
            }
            let Some(object_arguments) = prepare_quick_composed_object_arguments(
                eg,
                op_array,
                slot_base,
                cached,
                public_args,
                long_mask,
                object_mask,
                argument_plan,
            ) else {
                stats::inc_quick_loop_guard_failed();
                return Ok(QuickLoopOutcome::GuardFailed);
            };
            scalar_call_long_argument_mask = long_mask;
            scalar_call_object_arguments = object_arguments;
            if !scalar_call_typed_plan.is_null() {
                quick_composed_leaf_body = resolve_quick_composed_typed_body(
                    eg,
                    user,
                    &*scalar_call_typed_plan,
                    &scalar_call_object_arguments,
                    &mut quick_composed_targets,
                    &mut quick_composed_plans,
                    &mut quick_composed_string_plans,
                );
                if !quick_composed_leaf_body {
                    stats::inc_quick_loop_guard_failed();
                    return Ok(QuickLoopOutcome::GuardFailed);
                }
            } else {
                quick_composed_leaf_body = resolve_quick_composed_leaf_body(
                    eg,
                    user,
                    &*scalar_call_composed_plan,
                    &scalar_call_object_arguments,
                    &mut quick_composed_targets,
                    &mut quick_composed_plans,
                );
            }
        } else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
        scalar_call_common = cached;
        scalar_call_user = user;
    } else if let QuickLongTerm::ScalarCallTree {
        guard,
        argument_count,
        ..
    } = plan.term
    {
        let Some((cached, user)) = guarded_quick_scalar_call_target(
            eg,
            op_array,
            slot_base,
            guard,
            argument_count,
        ) else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        let user = &*user;
        let Some(method_plan) = user.scalar_long_plan.as_deref() else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        if method_plan.public_args != argument_count {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
        scalar_call_common = cached;
        scalar_call_plan = method_plan;
    }
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    if let QuickLongTerm::ScalarCallTree {
        guard, do_fcall_ip, ..
    } = plan.term
    {
        let initializer = op_array.instructions.as_ptr().add(guard.cache_ip());
        let Some(actual_do_fcall) = guard_quick_scalar_call_tree_generics(
            eg,
            frame,
            op_array,
            initializer,
            true,
            0,
        ) else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        if actual_do_fcall.offset_from(op_array.instructions.as_ptr()) != do_fcall_ip as isize {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
    }
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    if matches!(
        plan.term,
        QuickLongTerm::ScalarFunctionCall { .. } | QuickLongTerm::ScalarCallTree { .. }
    )
        && !scalar_call_common.is_null()
        && !scalar_call_plan.is_null()
        && let Some(outcome) = run_native_long_call_accumulate_loop(
            eg,
            frame,
            op_array,
            plan,
            scalar_call_common,
            &*scalar_call_plan,
            induction_ptr,
            accumulator_ptr,
            condition_ptr,
            tail_condition_ptr,
            term_ptr,
            sum_ptr,
            increment_ptr,
            induction,
            accumulator,
            bound,
        )?
    {
        return Ok(outcome);
    }
    let mut iterations = 0u64;
    let mut last_term = 0i64;
    let mut last_increment_result = 0i64;
    let mut completed_iteration = false;
    let mut scalar_call_targets =
        [std::ptr::null(); QUICK_SCALAR_MAX_RECORDED_CALLS];
    let mut scalar_call_target_count = 0usize;
    let mut scalar_call_success_count = 0u64;
    if scalar_call_fused_program.is_some() {
        // A fused leaf has no nested call targets. Its guarded outer target is
        // stable for this region activation, so prepare bookkeeping once
        // instead of reconstructing an empty composed-call list per iteration.
        scalar_call_targets[0] = scalar_call_common;
        scalar_call_target_count = 1;
    }
    // An elided scalar call represents its Init/Send/DoFcall protocol plus up
    // to eight arithmetic body operations. Check it every eight iterations;
    // ordinary accumulation retains the established 32-iteration cadence.
    let interrupt_iteration_mask = if scalar_call_common.is_null() { 31 } else { 7 };

    loop {
        if induction >= bound {
            Value::write_long(induction_ptr, induction);
            Value::write_long(accumulator_ptr, accumulator);
            if let Some(ptr) = condition_ptr {
                Value::write_bool(ptr, false);
            }
            if completed_iteration {
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, last_term);
                }
                if let Some(ptr) = term_destination_ptr {
                    Value::write_long(ptr, last_term);
                }
                Value::write_long(sum_ptr, accumulator);
                if let Some(ptr) = increment_ptr {
                    Value::write_long(ptr, last_increment_result);
                }
                if let (Some(guard), Some(ptr)) = (plan.tail_guard, tail_condition_ptr) {
                    Value::write_bool(ptr, guard.expected);
                }
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
            flush_quick_scalar_calls(
                &scalar_call_targets,
                scalar_call_target_count,
                &mut scalar_call_success_count,
            );
            stats::inc_quick_loop_completed(iterations);
            return Ok(QuickLoopOutcome::Completed);
        }

        if plan.tail_guard.is_none()
            && let QuickLongTerm::InductionPlusConst { addend, .. } = plan.term
            && let Some(chunk) = quick_induction_const_chunk(
                induction,
                accumulator,
                bound,
                addend,
                plan.increment_kind,
            )
        {
            induction = chunk.induction;
            accumulator = chunk.accumulator;
            last_term = chunk.last_term;
            last_increment_result = chunk.last_increment_result;
            completed_iteration = true;
            iterations += QUICK_INDUCTION_CONST_CHUNK as u64;

            if eg.vm_interrupt.load(Ordering::Relaxed) {
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, last_term);
                }
                if let Some(ptr) = term_destination_ptr {
                    Value::write_long(ptr, last_term);
                }
                Value::write_long(sum_ptr, accumulator);
                if let Some(ptr) = increment_ptr {
                    Value::write_long(ptr, last_increment_result);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
                handle_interrupt(eg)?;
            }
            continue;
        }

        let term = match &plan.term {
            QuickLongTerm::Induction => induction,
            QuickLongTerm::InductionPlusConst {
                addend, term_ip, ..
            } => match induction.checked_add(*addend) {
                Some(value) => value,
                None => {
                    Value::write_long(induction_ptr, induction);
                    Value::write_long(accumulator_ptr, accumulator);
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(*term_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongTerm::InductionPlusCv { term_ip, .. } => {
                let addend = invariant_addend.unwrap_unchecked();
                match induction.checked_add(addend) {
                    Some(value) => value,
                    None => {
                        Value::write_long(induction_ptr, induction);
                        Value::write_long(accumulator_ptr, accumulator);
                        if let Some(ptr) = condition_ptr {
                            Value::write_bool(ptr, true);
                        }
                        (*frame).opline = op_array.instructions.as_ptr().add(*term_ip);
                        stats::inc_quick_loop_deoptimized(iterations);
                        return Ok(QuickLoopOutcome::Deoptimized);
                    }
                }
            }
            QuickLongTerm::ArrayIndex {
                index, fetch_ip, ..
            } => {
                let fetched = match index {
                    QuickArrayIndex::Long(QuickLongOperand::Slot(_)) => {
                        quick_array.unwrap_unchecked().long_at_int(induction)
                    }
                    QuickArrayIndex::Long(QuickLongOperand::Const(_))
                    | QuickArrayIndex::StringLiteral(_)
                    | QuickArrayIndex::ValueSlot(_) => invariant_array_term,
                };
                let Some(fetched) = fetched else {
                    Value::write_long(induction_ptr, induction);
                    Value::write_long(accumulator_ptr, accumulator);
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(*fetch_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                };
                fetched
            }
            QuickLongTerm::StringLength { .. } => {
                invariant_string_length.unwrap_unchecked()
            }
            QuickLongTerm::AbsLong {
                operand_cv,
                term_ip,
                ..
            } => {
                if *operand_cv != plan.induction_cv {
                    invariant_abs.unwrap_unchecked()
                } else {
                    match induction.checked_abs() {
                        Some(value) => value,
                        None => {
                            Value::write_long(induction_ptr, induction);
                            Value::write_long(accumulator_ptr, accumulator);
                            if let Some(ptr) = condition_ptr {
                                Value::write_bool(ptr, true);
                            }
                            (*frame).opline =
                                op_array.instructions.as_ptr().add(*term_ip);
                            stats::inc_quick_loop_deoptimized(iterations);
                            return Ok(QuickLoopOutcome::Deoptimized);
                        }
                    }
                }
            }
            QuickLongTerm::ScalarFunctionCall {
                guard,
                argument_plan,
                argument_count,
                ..
            } => {
                debug_assert!(!scalar_call_common.is_null());
                debug_assert!(
                    !scalar_call_plan.is_null()
                        || !scalar_call_composed_plan.is_null()
                        || !scalar_call_typed_plan.is_null()
                );
                let evaluated = if let Some(program) = scalar_call_fused_program.as_ref() {
                    let result = evaluate_quick_fused_scalar_program(
                        program,
                        slot_base,
                        plan.induction_cv,
                        plan.accumulator_cv,
                        induction,
                        accumulator,
                    );
                    if result.is_some() {
                        scalar_call_success_count += 1;
                    }
                    result
                } else {
                    (|| {
                        let mut calls = [std::ptr::null(); COMPOSED_SCALAR_MAX_CALLS];
                        let mut call_count = 0usize;
                        let arguments = evaluate_quick_scalar_call_arguments(
                            argument_plan.as_ref(),
                            *argument_count,
                            scalar_call_long_argument_mask,
                            slot_base,
                            plan.induction_cv,
                            plan.accumulator_cv,
                            induction,
                            accumulator,
                        )?;
                        let result = if !scalar_call_plan.is_null() {
                            evaluate_scalar_long_plan(&*scalar_call_plan, &arguments)
                        } else if !scalar_call_typed_plan.is_null() {
                            evaluate_quick_composed_typed_body(
                                &*scalar_call_typed_plan,
                                &arguments,
                                &quick_composed_string_arguments,
                                &quick_composed_plans,
                                &quick_composed_string_plans,
                            )
                        } else if quick_composed_leaf_body {
                            evaluate_quick_composed_leaf_body(
                                &*scalar_call_composed_plan,
                                &arguments,
                                &quick_composed_plans,
                            )
                        } else {
                            debug_assert!(!scalar_call_user.is_null());
                            evaluate_composed_scalar_body_plan(
                                eg,
                                &*scalar_call_user,
                                &*scalar_call_composed_plan,
                                &arguments,
                                &scalar_call_object_arguments,
                                &mut calls,
                                &mut call_count,
                                0,
                            )
                        };
                        if result.is_some() {
                            if scalar_call_target_count == 0 {
                                if quick_composed_leaf_body {
                                    for called in quick_composed_targets
                                        .iter()
                                        .copied()
                                        .filter(|called| !called.is_null())
                                    {
                                        scalar_call_targets[scalar_call_target_count] = called;
                                        scalar_call_target_count += 1;
                                    }
                                } else {
                                    for called in calls.into_iter().take(call_count) {
                                        scalar_call_targets[scalar_call_target_count] = called;
                                        scalar_call_target_count += 1;
                                    }
                                }
                                scalar_call_targets[scalar_call_target_count] =
                                    scalar_call_common;
                                scalar_call_target_count += 1;
                            }
                            scalar_call_success_count += 1;
                        }
                        result
                    })()
                };
                let Some(value) = evaluated else {
                    Value::write_long(induction_ptr, induction);
                    Value::write_long(accumulator_ptr, accumulator);
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(guard.cache_ip());
                    flush_quick_scalar_calls(
                        &scalar_call_targets,
                        scalar_call_target_count,
                        &mut scalar_call_success_count,
                    );
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                };
                value
            }
            QuickLongTerm::ScalarCallTree {
                guard,
                do_fcall_ip,
                ..
            } => {
                debug_assert!(!scalar_call_common.is_null());
                debug_assert!(!scalar_call_plan.is_null());
                // The recursive call-tree evaluator reads caller CVs. Publish
                // the exact current loop state before entering it; these are
                // also the canonical values for transactional baseline replay.
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, accumulator);
                let mut calls = [std::ptr::null(); COMPOSED_SCALAR_MAX_CALLS];
                let mut call_count = 0usize;
                let initializer = op_array.instructions.as_ptr().add(guard.cache_ip());
                let evaluated = match evaluate_composed_scalar_call(
                    eg,
                    frame,
                    op_array,
                    initializer,
                    scalar_call_common,
                    &*scalar_call_plan,
                    &mut calls,
                    &mut call_count,
                    0,
                ) {
                    Some((value, do_fcall))
                        if do_fcall.offset_from(op_array.instructions.as_ptr())
                            == *do_fcall_ip as isize =>
                    {
                        if scalar_call_target_count == 0 {
                            for called in calls.into_iter().take(call_count) {
                                scalar_call_targets[scalar_call_target_count] = called;
                                scalar_call_target_count += 1;
                            }
                        }
                        scalar_call_success_count += 1;
                        Some(value)
                    }
                    _ => None,
                };
                let Some(value) = evaluated else {
                    Value::write_long(induction_ptr, induction);
                    Value::write_long(accumulator_ptr, accumulator);
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(guard.cache_ip());
                    flush_quick_scalar_calls(
                        &scalar_call_targets,
                        scalar_call_target_count,
                        &mut scalar_call_success_count,
                    );
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                };
                value
            }
        };

        let next_accumulator = match accumulator.checked_add(term) {
            Some(value) => value,
            None => {
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, term);
                }
                if let Some(ptr) = term_destination_ptr {
                    Value::write_long(ptr, term);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.sum_ip);
                flush_quick_scalar_calls(
                    &scalar_call_targets,
                    scalar_call_target_count,
                    &mut scalar_call_success_count,
                );
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };

        if let Some(guard) = plan.tail_guard {
            let operand = |operand| match operand {
                QuickLongOperand::Const(value) => value,
                QuickLongOperand::Slot(slot) if slot == plan.induction_cv => induction,
                QuickLongOperand::Slot(slot) if slot == plan.accumulator_cv => next_accumulator,
                QuickLongOperand::Slot(slot) => (*slot_base.add(slot as usize)).raw_long(),
            };
            let matches = apply_scalar_long_condition(
                guard.kind,
                operand(guard.lhs),
                operand(guard.rhs),
            ) == guard.expected;
            if !matches {
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, next_accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, term);
                }
                if let Some(ptr) = term_destination_ptr {
                    Value::write_long(ptr, term);
                }
                Value::write_long(sum_ptr, next_accumulator);
                (*frame).opline = op_array.instructions.as_ptr().add(guard.resume_ip);
                flush_quick_scalar_calls(
                    &scalar_call_targets,
                    scalar_call_target_count,
                    &mut scalar_call_success_count,
                );
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        }

        let next_induction = match induction.checked_add(1) {
            Some(value) => value,
            None => {
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, next_accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, term);
                }
                if let Some(ptr) = term_destination_ptr {
                    Value::write_long(ptr, term);
                }
                Value::write_long(sum_ptr, next_accumulator);
                if let (Some(guard), Some(ptr)) = (plan.tail_guard, tail_condition_ptr) {
                    Value::write_bool(ptr, guard.expected);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.increment_ip);
                flush_quick_scalar_calls(
                    &scalar_call_targets,
                    scalar_call_target_count,
                    &mut scalar_call_success_count,
                );
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            }
        };

        last_increment_result = match plan.increment_kind {
            QuickIncrementKind::Pre => next_induction,
            QuickIncrementKind::Post => induction,
        };
        last_term = term;
        induction = next_induction;
        accumulator = next_accumulator;
        completed_iteration = true;
        iterations += 1;

        if iterations & interrupt_iteration_mask == 0
            && eg.vm_interrupt.load(Ordering::Relaxed)
        {
            Value::write_long(induction_ptr, induction);
            Value::write_long(accumulator_ptr, accumulator);
            if let Some(ptr) = condition_ptr {
                Value::write_bool(ptr, true);
            }
            if let Some(ptr) = term_ptr {
                Value::write_long(ptr, last_term);
            }
            if let Some(ptr) = term_destination_ptr {
                Value::write_long(ptr, last_term);
            }
            Value::write_long(sum_ptr, accumulator);
            if let Some(ptr) = increment_ptr {
                Value::write_long(ptr, last_increment_result);
            }
            if let (Some(guard), Some(ptr)) = (plan.tail_guard, tail_condition_ptr) {
                Value::write_bool(ptr, guard.expected);
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
            flush_quick_scalar_calls(
                &scalar_call_targets,
                scalar_call_target_count,
                &mut scalar_call_success_count,
            );
            handle_interrupt(eg)?;
        }
    }
}

#[cfg(test)]
mod quick_induction_const_chunk_tests {
    use super::{
        QUICK_INDUCTION_CONST_CHUNK, QuickIncrementKind, quick_induction_const_chunk,
    };

    #[test]
    fn folds_one_interrupt_sized_positive_progression() {
        let post = quick_induction_const_chunk(0, 0, 100, 1, QuickIncrementKind::Post)
            .expect("positive progression");
        assert_eq!(post.induction, QUICK_INDUCTION_CONST_CHUNK);
        assert_eq!(post.accumulator, 528);
        assert_eq!(post.last_term, 32);
        assert_eq!(post.last_increment_result, 31);

        let pre = quick_induction_const_chunk(0, 0, 100, 1, QuickIncrementKind::Pre)
            .expect("positive progression");
        assert_eq!(pre.last_increment_result, 32);
    }

    #[test]
    fn leaves_short_sign_crossing_and_overflow_ranges_canonical() {
        assert!(
            quick_induction_const_chunk(0, 0, 31, 1, QuickIncrementKind::Post).is_none()
        );
        assert!(
            quick_induction_const_chunk(-16, 0, 100, 0, QuickIncrementKind::Post).is_none()
        );
        assert!(
            quick_induction_const_chunk(
                i64::MAX - 31,
                0,
                i64::MAX,
                1,
                QuickIncrementKind::Post,
            )
            .is_none()
        );
    }
}
