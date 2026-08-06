// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_long_ops_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs
        || (*frame).num_cvs + (*frame).num_temps > 64
        || (*frame).heap_bitmap
            & (plan.involved_mask
                & !(plan.array_input_mask
                    | plan.array_output_mask
                    | plan.string_input_mask
                    | plan.string_append_mask
                    | plan.object_input_mask))
            != 0
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    if let Some(kernel) = plan.straight_array_kernel {
        return Ok(run_quick_straight_array_region(
            frame,
            op_array,
            plan,
            kernel,
        ));
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let mut slots = [0i64; 64];
    let invariant_long_output_mask = plan
        .typed_invariant_source
        .as_ref()
        .map_or(0, |source| source.long_output_mask);
    let mut input_mask = plan.long_input_mask & !invariant_long_output_mask;
    while input_mask != 0 {
        let slot = input_mask.trailing_zeros() as usize;
        input_mask &= input_mask - 1;
        let value = slot_base.add(slot);
        if (*value).value_type() != ValueType::Long {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
        slots[slot] = (*value).raw_long();
    }

    let mut string_mask = plan.string_input_mask;
    while string_mask != 0 {
        let slot = string_mask.trailing_zeros() as usize;
        string_mask &= string_mask - 1;
        if (*slot_base.add(slot)).value_type() != ValueType::String {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
    }

    let mut object_mask = plan.object_input_mask;
    while object_mask != 0 {
        let slot = object_mask.trailing_zeros() as usize;
        object_mask &= object_mask - 1;
        let value = &*slot_base.add(slot);
        if value.value_type() != ValueType::Object
            || value.is_reference()
        {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        }
    }

    if !prepare_quick_typed_invariant_source(
        frame,
        op_array,
        plan.typed_invariant_source.as_ref(),
        slot_base,
    ) {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }
    let mut invariant_outputs = invariant_long_output_mask;
    while invariant_outputs != 0 {
        let slot = invariant_outputs.trailing_zeros() as usize;
        invariant_outputs &= invariant_outputs - 1;
        slots[slot] = (*slot_base.add(slot)).raw_long();
    }

    if let Some(kernel) = quick_long_branch_only_kernel(plan) {
        return run_quick_long_branch_only_kernel(
            eg, frame, op_array, plan, slot_base, slots, kernel,
        );
    }

    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    if let Some(kernel) = native_quick_long_straight_kernel(plan) {
        if let Some(outcome) = run_native_quick_long_straight_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            &mut slots,
            &kernel,
        )? {
            #[cfg(feature = "vm-stats")]
            record_native_quick_outcome(
                stats::JitRegionKind::TypedOpsLoop,
                &outcome,
            );
            return Ok(outcome);
        }
    }

    if let Some((kernel, body)) = quick_long_conditional_kernel(plan) {
        return dispatch_quick_long_conditional_kernel(
            eg, frame, op_array, plan, slot_base, slots, kernel, body,
        );
    }

    let mut mutable_strings = [std::ptr::null_mut(); 64];
    let mut string_append_mask = plan.string_append_mask;
    while string_append_mask != 0 {
        let slot = string_append_mask.trailing_zeros() as usize;
        string_append_mask &= string_append_mask - 1;
        let value = &mut *slot_base.add(slot);
        let Some(string) = value.as_string_mut_if_unique() else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        mutable_strings[slot] = string;
    }

    let mut mutable_arrays = [std::ptr::null_mut(); 64];
    let mut array_output_mask = plan.array_output_mask;
    while array_output_mask != 0 {
        let slot = array_output_mask.trailing_zeros() as usize;
        array_output_mask &= array_output_mask - 1;
        let value = &mut *slot_base.add(slot);
        let Some(array) = value.as_array_mut_if_unique() else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        mutable_arrays[slot] = array;
    }

    let array_kernel = quick_long_array_loop_kernel(plan);
    let exact_array_slot = array_kernel.as_ref().and_then(|(kernel, body, _)| {
        matches!(body, QuickLongArrayBodyKernel::OneAdd { .. })
            .then_some(kernel.array as usize)
    });
    let mut arrays = [QuickLongArray::EMPTY; 64];
    let mut exact_int_layout = None;
    let mut int_position_hints = [None; 64];
    let mut indexed_int_array_mask = 0u64;
    let mut array_mask = plan.array_input_mask;
    while array_mask != 0 {
        let slot = array_mask.trailing_zeros() as usize;
        array_mask &= array_mask - 1;
        let value = &*slot_base.add(slot);
        let Some(array) = value.as_array() else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        let quick_array = QuickLongArray::from_array(array);
        if matches!(quick_array, QuickLongArray::Hash { .. }) {
            if exact_array_slot == Some(slot)
                && let Some(layout) = array.exact_ordered_int_layout()
            {
                exact_int_layout = Some(QuickLongExactIntLayout { layout });
            } else if let Some((first_key, stride)) = array.integer_position_hint() {
                int_position_hints[slot] =
                    Some(QuickLongIntPositionHint { first_key, stride });
            } else {
                indexed_int_array_mask |= 1u64 << slot;
            }
        }
        arrays[slot] = quick_array;
    }

    if let Some((kernel, body, prefix)) = array_kernel {
        if !prefix.is_empty() {
            return dispatch_quick_long_composed_array_loop_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                &arrays,
                &int_position_hints,
                indexed_int_array_mask,
                exact_int_layout,
                kernel,
                body,
                &prefix,
            );
        }
        if let (
            Some(exact_layout),
            QuickLongArray::Hash { array },
            QuickArrayIndex::Long(index),
            QuickLongArrayBodyKernel::OneAdd { add },
        ) = (
            exact_int_layout,
            arrays[kernel.array as usize],
            kernel.index,
            body,
        ) {
            return run_quick_long_exact_int_array_one_add_kernel(
                eg,
                frame,
                op_array,
                plan,
                slot_base,
                slots,
                exact_layout,
                array,
                index,
                kernel,
                add,
            );
        }
        return dispatch_quick_long_array_loop_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            &arrays,
            &int_position_hints,
            indexed_int_array_mask,
            kernel,
            body,
        );
    }

    let mut string_state = QuickStringSlotState::new(slot_base, plan.string_input_mask);
    let resolved_object_ops = if plan.object_input_mask == 0 {
        Vec::new()
    } else {
        let Some(resolved) = resolve_quick_object_ops(
            eg,
            op_array,
            slot_base,
            &slots,
            &string_state,
            plan,
        ) else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        resolved
    };
    let invariant_object_property_mask = if plan.object_input_mask == 0 {
        0
    } else {
        let Some(mask) = prepare_quick_invariant_object_properties(
            plan,
            &resolved_object_ops,
            &mut slots,
        ) else {
            stats::inc_quick_loop_guard_failed();
            return Ok(QuickLoopOutcome::GuardFailed);
        };
        mask
    };
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    if let Some(kernel) = native_quick_long_mixed_kernel(
        op_array,
        plan,
        &resolved_object_ops,
    ) {
        if let Some(outcome) = run_native_quick_long_mixed_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            &mut slots,
            &mutable_arrays,
            &mut string_state,
            &resolved_object_ops,
            &kernel,
        )? {
            #[cfg(feature = "vm-stats")]
            record_native_quick_outcome(
                stats::JitRegionKind::TypedOpsLoop,
                &outcome,
            );
            return Ok(outcome);
        }
    }
    if let Some(kernel) = quick_long_invariant_property_accumulate_kernel(
        plan,
        invariant_object_property_mask,
    ) {
        return run_quick_long_invariant_property_accumulate_kernel(
            eg,
            frame,
            op_array,
            plan,
            slot_base,
            slots,
            kernel,
        );
    }
    let mut object_call_recorder = QuickObjectCallRecorder {
        counts: vec![0; resolved_object_ops.len()],
        resolved: &resolved_object_ops,
    };

    let mut string_fetch_cache = QuickStringFetchCache::new(plan.string_cache_capacity);
    let mut dirty_long_mask = 0u64;
    let mut dirty_bool_mask = 0u64;
    let mut iterations = 0u64;
    let mut op_index = plan.entry_op as usize;

    loop {
        let mut completed_backedge = false;
        let next_target = match *plan.ops.get_unchecked(op_index) {
            QuickLongOp::BranchUnlessLt {
                lhs,
                rhs,
                condition_tmp,
                false_target,
                next_target,
                ..
            } => {
                let rhs = match rhs {
                    QuickLongOperand::Slot(slot) => slots[slot as usize],
                    QuickLongOperand::Const(value) => value,
                };
                let condition = slots[lhs as usize] < rhs;
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    dirty_bool_mask |= 1u64 << slot;
                }
                if condition { next_target } else { false_target }
            }
            QuickLongOp::BranchUnlessEq {
                lhs,
                rhs,
                condition_tmp,
                false_target,
                next_target,
                ..
            } => {
                let rhs = match rhs {
                    QuickLongOperand::Slot(slot) => slots[slot as usize],
                    QuickLongOperand::Const(value) => value,
                };
                let condition = slots[lhs as usize] == rhs;
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    dirty_bool_mask |= 1u64 << slot;
                }
                if condition { next_target } else { false_target }
            }
            QuickLongOp::BranchUnlessLe {
                lhs,
                rhs,
                condition_tmp,
                false_target,
                next_target,
                ..
            } => {
                let condition = quick_long_operand(&slots, lhs)
                    <= quick_long_operand(&slots, rhs);
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    dirty_bool_mask |= 1u64 << slot;
                }
                if condition { next_target } else { false_target }
            }
            QuickLongOp::TraceGuard {
                kind,
                lhs,
                rhs,
                expected,
                condition_tmp,
                next_target,
                resume_ip,
            } => {
                let condition = apply_scalar_long_condition(
                    kind,
                    quick_long_operand(&slots, lhs),
                    quick_long_operand(&slots, rhs),
                );
                if condition != expected {
                    string_state.commit();
                    return Ok(deopt_quick_long_kernel(
                        frame,
                        op_array,
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        resume_ip,
                        iterations,
                    ));
                }
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    dirty_bool_mask |= 1u64 << slot;
                }
                next_target
            }
            QuickLongOp::ModConst {
                value,
                divisor,
                result,
                next_target,
                resume_ip,
            } => match slots[value as usize].checked_rem(divisor) {
                Some(remainder) => {
                    slots[result as usize] = remainder;
                    dirty_long_mask |= 1u64 << result;
                    next_target
                }
                None => {
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
            },
            QuickLongOp::JsonProjectionStep { next_target, .. } => next_target,
            QuickLongOp::FetchArrayLong {
                array,
                index,
                result,
                destination,
                next_target,
                resume_ip,
            } => {
                if let Some(fusion) = *plan.array_update_fusions.get_unchecked(op_index) {
                    let entry = match index {
                        QuickArrayIndex::ValueSlot(slot) => {
                            let key = string_state.value(slot).as_str().unwrap_unchecked();
                            string_fetch_cache.long_entry_at_mut(
                                array,
                                mutable_arrays[array as usize],
                                key,
                            )
                        }
                        _ => mutable_long_entry_at(
                            mutable_arrays[array as usize],
                            index,
                            &slots,
                            op_array,
                        ),
                    };
                    let Some((fetched, value_ptr)) = entry else {
                        commit_quick_long_ops_slots(
                            slot_base,
                            &slots,
                            dirty_long_mask,
                            dirty_bool_mask,
                        );
                        (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                        stats::inc_quick_loop_deoptimized(iterations);
                        return Ok(QuickLoopOutcome::Deoptimized);
                    };
                    debug_assert!(!value_ptr.is_null());
                    slots[result as usize] = fetched;
                    dirty_long_mask |= 1u64 << result;
                    if let Some(destination) = destination {
                        slots[destination as usize] = fetched;
                        dirty_long_mask |= 1u64 << destination;
                    }

                    let Some(stored) = apply_scalar_long_op(
                        fusion.kind,
                        quick_long_operand(&slots, fusion.lhs),
                        quick_long_operand(&slots, fusion.rhs),
                    ) else {
                        commit_quick_long_ops_slots(
                            slot_base,
                            &slots,
                            dirty_long_mask,
                            dirty_bool_mask,
                        );
                        (*frame).opline = op_array
                            .instructions
                            .as_ptr()
                            .add(fusion.arithmetic_resume_ip);
                        stats::inc_quick_loop_deoptimized(iterations);
                        return Ok(QuickLoopOutcome::Deoptimized);
                    };
                    slots[fusion.result as usize] = stored;
                    dirty_long_mask |= 1u64 << fusion.result;
                    Value::write_long(value_ptr, stored);
                    if let QuickArrayIndex::ValueSlot(slot) = index {
                        let key = string_state.value(slot).as_str().unwrap_unchecked();
                        string_fetch_cache.store_long(array, key, stored);
                    }
                    fusion.next_target
                } else {
                let fetched = match index {
                    QuickArrayIndex::ValueSlot(slot) => {
                        let key = string_state.value(slot).as_str().unwrap_unchecked();
                        string_fetch_cache.long_at(array, arrays[array as usize], key)
                    }
                    _ => arrays[array as usize].long_at(index, &slots, op_array),
                };
                let Some(fetched) = fetched else {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                };
                slots[result as usize] = fetched;
                dirty_long_mask |= 1u64 << result;
                if let Some(destination) = destination {
                    slots[destination as usize] = fetched;
                    dirty_long_mask |= 1u64 << destination;
                }
                next_target
                }
            }
            QuickLongOp::StoreArrayLong {
                array,
                index,
                value,
                next_target,
                ..
            } => {
                let stored = slots[value as usize];
                let array_ptr = mutable_arrays[array as usize];
                debug_assert!(!array_ptr.is_null());
                match index {
                    QuickArrayIndex::Long(index) => {
                        (*array_ptr).set_int(
                            quick_long_operand(&slots, index),
                            Value::long(stored),
                        );
                    }
                    QuickArrayIndex::StringLiteral(literal) => {
                        let key = op_array
                            .literals
                            .get_unchecked(literal as usize)
                            .as_str()
                            .unwrap_unchecked();
                        if let Some(key) = canonical_decimal_array_key(key) {
                            (*array_ptr).set_int(key, Value::long(stored));
                        } else {
                            (*array_ptr).set_str(key, Value::long(stored));
                        }
                    }
                    QuickArrayIndex::ValueSlot(slot) => {
                        let key = string_state.value(slot).as_str().unwrap_unchecked();
                        if let Some(normalized) = canonical_decimal_array_key(key) {
                            (*array_ptr).set_int(normalized, Value::long(stored));
                        } else {
                            (*array_ptr).set_str(key, Value::long(stored));
                        }
                        string_fetch_cache.store_long(array, key, stored);
                    }
                }
                next_target
            }
            QuickLongOp::ArrayPushLong {
                array,
                value,
                next_target,
                ..
            } => {
                let value = match value {
                    QuickLongOperand::Slot(slot) => slots[slot as usize],
                    QuickLongOperand::Const(value) => value,
                };
                let array = mutable_arrays[array as usize];
                debug_assert!(!array.is_null());
                (*array).push(Value::long(value));
                next_target
            }
            QuickLongOp::StringAppend {
                destination,
                source,
                next_target,
                ..
            } => {
                let source = match source {
                    QuickStringAppendSource::Literal(literal) => op_array
                        .literals
                        .get_unchecked(literal as usize)
                        .as_str()
                        .unwrap_unchecked(),
                    QuickStringAppendSource::Slot(slot) => {
                        (*slot_base.add(slot as usize)).as_str().unwrap_unchecked()
                    }
                };
                let destination = mutable_strings[destination as usize];
                debug_assert!(!destination.is_null());
                (*destination).push_str(source);
                next_target
            }
            QuickLongOp::Add {
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } => match slots[lhs as usize].checked_add(slots[rhs as usize]) {
                Some(value) => {
                    slots[result as usize] = value;
                    dirty_long_mask |= 1u64 << result;
                    next_target
                }
                None => {
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
            },
            QuickLongOp::Binary {
                kind,
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } => match apply_scalar_long_op(
                kind,
                quick_long_operand(&slots, lhs),
                quick_long_operand(&slots, rhs),
            ) {
                Some(value) => {
                    slots[result as usize] = value;
                    dirty_long_mask |= 1u64 << result;
                    next_target
                }
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    string_state.commit();
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongOp::BinaryAssign {
                kind,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                resume_ip,
            } => match apply_scalar_long_op(
                kind,
                quick_long_operand(&slots, lhs),
                quick_long_operand(&slots, rhs),
            ) {
                Some(value) => {
                    slots[result as usize] = value;
                    slots[destination as usize] = value;
                    dirty_long_mask |= (1u64 << result) | (1u64 << destination);
                    next_target
                }
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    string_state.commit();
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongOp::AddAssign {
                lhs,
                rhs,
                result,
                destination,
                next_target,
                add_resume_ip,
            } => match slots[lhs as usize].checked_add(slots[rhs as usize]) {
                Some(value) => {
                    slots[result as usize] = value;
                    slots[destination as usize] = value;
                    dirty_long_mask |= (1u64 << result) | (1u64 << destination);
                    next_target
                }
                None => {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    (*frame).opline =
                        op_array.instructions.as_ptr().add(add_resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                }
            },
            QuickLongOp::ConditionalAddAssign {
                condition,
                condition_tmp,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                add_resume_ip,
                ..
            } => {
                let condition = match condition {
                    QuickLongCondition::Lt { lhs, rhs } => {
                        let rhs = match rhs {
                            QuickLongOperand::Slot(slot) => slots[slot as usize],
                            QuickLongOperand::Const(value) => value,
                        };
                        slots[lhs as usize] < rhs
                    }
                    QuickLongCondition::Eq { lhs, rhs } => {
                        let rhs = match rhs {
                            QuickLongOperand::Slot(slot) => slots[slot as usize],
                            QuickLongOperand::Const(value) => value,
                        };
                        slots[lhs as usize] == rhs
                    }
                };
                if let Some(slot) = condition_tmp {
                    slots[slot as usize] = i64::from(condition);
                    dirty_bool_mask |= 1u64 << slot;
                }
                if !condition {
                    next_target
                } else {
                    match slots[lhs as usize].checked_add(slots[rhs as usize]) {
                        Some(value) => {
                            slots[result as usize] = value;
                            slots[destination as usize] = value;
                            dirty_long_mask |=
                                (1u64 << result) | (1u64 << destination);
                            next_target
                        }
                        None => {
                            commit_quick_long_ops_slots(
                                slot_base,
                                &slots,
                                dirty_long_mask,
                                dirty_bool_mask,
                            );
                            (*frame).opline =
                                op_array.instructions.as_ptr().add(add_resume_ip);
                            stats::inc_quick_loop_deoptimized(iterations);
                            return Ok(QuickLoopOutcome::Deoptimized);
                        }
                    }
                }
            }
            QuickLongOp::AddAddAssign {
                first_lhs,
                first_rhs,
                first_result,
                second_lhs,
                second_rhs,
                second_result,
                destination,
                next_target,
                first_resume_ip,
                second_resume_ip,
            } => {
                let first = match slots[first_lhs as usize]
                    .checked_add(slots[first_rhs as usize])
                {
                    Some(value) => value,
                    None => {
                        commit_quick_long_ops_slots(
                            slot_base,
                            &slots,
                            dirty_long_mask,
                            dirty_bool_mask,
                        );
                        (*frame).opline =
                            op_array.instructions.as_ptr().add(first_resume_ip);
                        stats::inc_quick_loop_deoptimized(iterations);
                        return Ok(QuickLoopOutcome::Deoptimized);
                    }
                };
                slots[first_result as usize] = first;
                dirty_long_mask |= 1u64 << first_result;

                let second = match slots[second_lhs as usize]
                    .checked_add(slots[second_rhs as usize])
                {
                    Some(value) => value,
                    None => {
                        commit_quick_long_ops_slots(
                            slot_base,
                            &slots,
                            dirty_long_mask,
                            dirty_bool_mask,
                        );
                        (*frame).opline =
                            op_array.instructions.as_ptr().add(second_resume_ip);
                        stats::inc_quick_loop_deoptimized(iterations);
                        return Ok(QuickLoopOutcome::Deoptimized);
                    }
                };
                slots[second_result as usize] = second;
                slots[destination as usize] = second;
                dirty_long_mask |=
                    (1u64 << second_result) | (1u64 << destination);
                next_target
            }
            QuickLongOp::ObjectPropertyLong {
                result,
                next_target,
                resume_ip,
                ..
            } => {
                if invariant_object_property_mask & (1u64 << result) == 0 {
                    let QuickResolvedObjectOp::PropertyRead { property } =
                        *resolved_object_ops.get_unchecked(op_index)
                    else {
                        unreachable!("resolved object property read")
                    };
                    if (*property).value_type() != ValueType::Long || (*property).is_reference() {
                        string_state.commit();
                        return Ok(deopt_quick_long_kernel(
                            frame,
                            op_array,
                            slot_base,
                            &slots,
                            dirty_long_mask,
                            dirty_bool_mask,
                            resume_ip,
                            iterations,
                        ));
                    }
                    slots[result as usize] = (*property).raw_long();
                }
                dirty_long_mask |= 1u64 << result;
                next_target
            }
            QuickLongOp::ObjectPropertyStringLength {
                result,
                next_target,
                resume_ip,
                ..
            } => {
                if invariant_object_property_mask & (1u64 << result) == 0 {
                    let QuickResolvedObjectOp::PropertyRead { property } =
                        *resolved_object_ops.get_unchecked(op_index)
                    else {
                        unreachable!("resolved object property strlen")
                    };
                    let Some(value) = (*property).as_str() else {
                        string_state.commit();
                        return Ok(deopt_quick_long_kernel(
                            frame,
                            op_array,
                            slot_base,
                            &slots,
                            dirty_long_mask,
                            dirty_bool_mask,
                            resume_ip,
                            iterations,
                        ));
                    };
                    slots[result as usize] = value.len() as i64;
                }
                dirty_long_mask |= 1u64 << result;
                next_target
            }
            QuickLongOp::PropertyMethodCall { call } => {
                let QuickResolvedObjectOp::PropertyMethod {
                    receiver,
                    plan,
                    property_slots,
                    property_count,
                    ..
                } = *resolved_object_ops.get_unchecked(op_index)
                else {
                    unreachable!("resolved property method operation")
                };
                let arguments = quick_typed_method_arguments(&slots, &call);
                if try_execute_resolved_long_property_plan(
                    &*receiver,
                    &arguments,
                    &*plan,
                    &property_slots,
                    property_count,
                ) {
                    object_call_recorder.record(op_index);
                    call.next_target
                } else {
                    return Ok(deopt_quick_typed_method_call(
                        frame,
                        op_array,
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        &mut string_state,
                        call,
                        iterations,
                    ));
                }
            }
            QuickLongOp::PropertyGetterCall { call, result } => {
                let QuickResolvedObjectOp::PropertyGetter {
                    receiver,
                    property_slot,
                    ..
                } = *resolved_object_ops.get_unchecked(op_index)
                else {
                    unreachable!("resolved property getter operation")
                };
                let property = &*(*receiver).object_property_slot_unchecked(property_slot);
                if property.value_type() == ValueType::Long && !property.is_reference() {
                    slots[result as usize] = property.raw_long();
                    dirty_long_mask |= 1u64 << result;
                    object_call_recorder.record(op_index);
                    call.next_target
                } else {
                    return Ok(deopt_quick_typed_method_call(
                        frame,
                        op_array,
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        &mut string_state,
                        call,
                        iterations,
                    ));
                }
            }
            QuickLongOp::ScalarMethodCall { call, result } => {
                let arguments = quick_typed_method_arguments(&slots, &call);
                let value = match *resolved_object_ops.get_unchecked(op_index) {
                    QuickResolvedObjectOp::ScalarMethod { plan, .. } => {
                        evaluate_scalar_long_plan(&*plan, &arguments)
                    }
                    QuickResolvedObjectOp::ObjectLongMethod {
                        receiver,
                        user,
                        plan,
                        ..
                    } => {
                        let object_arguments =
                            call.arguments.map(QuickObjectLongArgument::Long);
                        evaluate_quick_object_long_method(
                            receiver,
                            user,
                            plan,
                            &object_arguments,
                            call.argument_count,
                            &slots,
                            &string_state,
                        )
                    }
                    _ => unreachable!("resolved scalar method operation"),
                };
                if let Some(value) = value {
                    slots[result as usize] = value;
                    dirty_long_mask |= 1u64 << result;
                    object_call_recorder.record(op_index);
                    call.next_target
                } else {
                    return Ok(deopt_quick_typed_method_call(
                        frame,
                        op_array,
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        &mut string_state,
                        call,
                        iterations,
                    ));
                }
            }
            QuickLongOp::ObjectLongMethodCall { call, result } => {
                let value = match *resolved_object_ops.get_unchecked(op_index) {
                    QuickResolvedObjectOp::ObjectLongMethod {
                        receiver,
                        user,
                        plan,
                        ..
                    } => evaluate_quick_object_long_method(
                        receiver,
                        user,
                        plan,
                        &call.arguments,
                        call.argument_count,
                        &slots,
                        &string_state,
                    ),
                    QuickResolvedObjectOp::ComposedTypedMethod { plan, .. } => {
                        evaluate_quick_composed_typed_method(
                            plan,
                            &call.arguments,
                            call.argument_count,
                            &slots,
                            &string_state,
                        )
                    }
                    _ => unreachable!("resolved object-long method operation"),
                };
                if let Some(value) = value {
                    slots[result as usize] = value;
                    dirty_long_mask |= 1u64 << result;
                    object_call_recorder.record(op_index);
                    call.next_target
                } else {
                    string_state.commit();
                    return Ok(deopt_quick_long_kernel(
                        frame,
                        op_array,
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                        call.resume_ip,
                        iterations,
                    ));
                }
            }
            QuickLongOp::ComposedPropertyCall {
                next_target,
                resume_ip,
                ..
            } => {
                let QuickResolvedObjectOp::ComposedProperty {
                    outer_receiver,
                    outer_user,
                    outer_plan,
                    inner_receiver,
                    inner_property_slot,
                    ..
                } = *resolved_object_ops.get_unchecked(op_index)
                else {
                    unreachable!("resolved composed property operation")
                };
                let property =
                    &*(*inner_receiver).object_property_slot_unchecked(inner_property_slot);
                let mut arguments = [0i64; 8];
                if property.value_type() == ValueType::Long && !property.is_reference() {
                    arguments[0] = property.raw_long();
                } else {
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
                if try_execute_long_property_plan(
                    &*outer_receiver,
                    &arguments,
                    &*outer_plan,
                    &*outer_user,
                ) {
                    object_call_recorder.record(op_index);
                    next_target
                } else {
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
            }
            QuickLongOp::VirtualObjectArrayPipeline {
                constructor_arguments,
                argument_count: _,
                consumers,
                consumer_count,
                trailing_key_literal,
                trailing_result,
                output_mask,
                next_target,
                resume_ip,
            } => {
                let QuickResolvedObjectOp::VirtualPipeline { pipeline } =
                    *resolved_object_ops.get_unchecked(op_index)
                else {
                    unreachable!("resolved virtual pipeline operation")
                };
                let evaluated = try_execute_resolved_quick_virtual_pipeline(
                    eg,
                    op_array,
                    &mut slots,
                    &string_state,
                    pipeline,
                    &constructor_arguments,
                    &consumers,
                    consumer_count,
                    trailing_key_literal,
                    trailing_result,
                );
                let Some(evaluated) = evaluated else {
                    commit_quick_long_ops_slots(
                        slot_base,
                        &slots,
                        dirty_long_mask,
                        dirty_bool_mask,
                    );
                    string_state.commit();
                    (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                    stats::inc_quick_loop_deoptimized(iterations);
                    return Ok(QuickLoopOutcome::Deoptimized);
                };
                evaluated.record_calls();
                object_call_recorder.record(op_index);
                dirty_long_mask |= output_mask;
                next_target
            }
            QuickLongOp::Assign {
                destination,
                source,
                next_target,
            } => {
                slots[destination as usize] = slots[source as usize];
                dirty_long_mask |= 1u64 << destination;
                next_target
            }
            QuickLongOp::AssignLongLiteral {
                destination,
                value,
                next_target,
            } => {
                slots[destination as usize] = value;
                dirty_long_mask |= 1u64 << destination;
                next_target
            }
            QuickLongOp::AssignStringLiteral {
                destination,
                literal,
                next_target,
            } => {
                let value = op_array.literals.as_ptr().add(literal as usize);
                debug_assert_eq!((*value).value_type(), ValueType::String);
                string_state.assign_literal(destination, value);
                next_target
            }
            QuickLongOp::AssignStringSlot {
                destination,
                source,
                next_target,
            } => {
                string_state.assign_slot(destination, source);
                next_target
            }
            QuickLongOp::PostInc {
                value,
                result,
                next_target,
                resume_ip,
            }
            | QuickLongOp::PostIncJump {
                value,
                result,
                target: next_target,
                resume_ip,
            } => match slots[value as usize].checked_add(1) {
                Some(incremented) => {
                    if let Some(result) = result {
                        slots[result as usize] = slots[value as usize];
                        dirty_long_mask |= 1u64 << result;
                    }
                    slots[value as usize] = incremented;
                    dirty_long_mask |= 1u64 << value;
                    next_target
                }
                None => {
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
            },
            QuickLongOp::PostIncLoopLt {
                value,
                result,
                condition_lhs,
                condition_rhs,
                condition_tmp,
                body_target,
                exit_target,
                resume_ip,
            } => match slots[value as usize].checked_add(1) {
                Some(incremented) => {
                    if let Some(result) = result {
                        slots[result as usize] = slots[value as usize];
                        dirty_long_mask |= 1u64 << result;
                    }
                    slots[value as usize] = incremented;
                    dirty_long_mask |= 1u64 << value;

                    let rhs = match condition_rhs {
                        QuickLongOperand::Slot(slot) => slots[slot as usize],
                        QuickLongOperand::Const(value) => value,
                    };
                    let condition = slots[condition_lhs as usize] < rhs;
                    if let Some(slot) = condition_tmp {
                        slots[slot as usize] = i64::from(condition);
                        dirty_bool_mask |= 1u64 << slot;
                    }
                    completed_backedge = true;
                    if condition { body_target } else { exit_target }
                }
                None => {
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
            },
            QuickLongOp::Jump { target } => target,
        };

        if completed_backedge || next_target.op_index() == Some(plan.entry_op as usize) {
            iterations += 1;
            if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
                commit_quick_long_ops_slots(
                    slot_base,
                    &slots,
                    dirty_long_mask,
                    dirty_bool_mask,
                );
                string_state.commit();
                let next_ip = plan.target_ip(next_target).unwrap_unchecked();
                (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
                object_call_recorder.flush();
                handle_interrupt(eg)?;
            }
        }

        if let Some(next_index) = next_target.op_index() {
            op_index = next_index;
            continue;
        }

        commit_quick_long_ops_slots(
            slot_base,
            &slots,
            dirty_long_mask,
            dirty_bool_mask,
        );
        let next_ip = next_target.exit_ip().unwrap_unchecked();
        (*frame).opline = op_array.instructions.as_ptr().add(next_ip);
        stats::inc_quick_loop_completed(iterations);
        return Ok(QuickLoopOutcome::Completed);
    }
}
