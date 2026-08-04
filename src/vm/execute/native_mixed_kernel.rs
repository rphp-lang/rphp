// Kept in the execute module through include! so this structural split does not change visibility or code generation.

unsafe fn native_quick_long_mixed_kernel(
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    resolved_object_ops: &[QuickResolvedObjectOp],
) -> Option<NativeQuickLongMixedKernel> {
    if plan.entry_op != 0
        || plan.ops.len() < 3
        || plan.ops.len() > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 2
    {
        return None;
    }
    let (
        header_lhs,
        header_rhs,
        header_condition_tmp,
        header_false_target,
        header_next_target,
    ) = match *plan.ops.first()? {
        QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        } => (lhs, rhs, condition_tmp, false_target, next_target),
        _ => return None,
    };
    header_false_target.exit_ip()?;
    let (
        post_value,
        post_result,
        post_condition_lhs,
        post_condition_rhs,
        post_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
    ) = match *plan.ops.last()? {
        QuickLongOp::PostIncLoopLt {
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        } => (
            value,
            result,
            condition_lhs,
            condition_rhs,
            condition_tmp,
            body_target,
            exit_target,
            resume_ip,
        ),
        _ => return None,
    };
    if header_lhs != post_value
        || header_next_target.op_index() != Some(1)
        || body_target.op_index() != Some(1)
        || exit_target != header_false_target
        || post_condition_lhs != header_lhs
        || post_condition_rhs != header_rhs
        || post_condition_tmp != header_condition_tmp
        || post_result == Some(post_value)
    {
        return None;
    }

    let mut string_literals = [0u16; NATIVE_FINITE_STRING_LIMIT];
    let mut string_lengths = [0i64; NATIVE_FINITE_STRING_LIMIT];
    let mut string_token_count = 0usize;
    for operation in plan.ops.iter().copied() {
        let QuickLongOp::AssignStringLiteral { literal, .. } = operation else {
            continue;
        };
        if string_literals[..string_token_count].contains(&literal) {
            continue;
        }
        if string_token_count == NATIVE_FINITE_STRING_LIMIT {
            return None;
        }
        string_literals[string_token_count] = literal;
        string_lengths[string_token_count] = i64::try_from(
            op_array.literals.get(literal as usize)?.as_str()?.len(),
        )
        .ok()?;
        string_token_count += 1;
    }
    let mut builder = NativeMixedBuildState {
        operations: [NativeStraightLongOperation::Unused;
            NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
        operation_resume_ips: [0; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
        operation_count: 0,
        used_slots: plan.involved_mask,
        string_literals,
        string_lengths,
        string_token_count,
        context_array_slots: [0; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
        context_tokens: [0; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
        context_count: 0,
        property_binding_op_indices: [0; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
        property_binding_property_indices: [0; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
        property_binding_slots: [0; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
        property_binding_receivers: [std::ptr::null();
            NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
        property_binding_object_slots: [usize::MAX;
            NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
        property_binding_count: 0,
        call_targets: [std::ptr::null(); NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
        call_completion_operations: [0; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
        call_count: 0,
    };
    let body_end = plan.ops.len() - 1;
    let mut plan_to_native = vec![u8::MAX; plan.ops.len()];
    let mut pending_branches = Vec::new();
    let mut pending_jumps = Vec::new();
    let mut trace_guard_operation_indices = [0u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_condition_slots = [0u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_expected = [false; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let mut trace_guard_count = 0usize;
    let mut has_hash_update = false;
    let mut has_virtual_pipeline = false;
    let mut has_property_method = false;
    let mut has_typed_method = false;
    let mut plan_index = 1usize;

    while plan_index < body_end {
        plan_to_native[plan_index] = u8::try_from(builder.operation_count).ok()?;
        match plan.ops[plan_index] {
            QuickLongOp::BranchUnlessLt {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                let target = false_target.op_index()?;
                let native_index = builder.append(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::LessThan,
                        lhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Slot(lhs),
                        ),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
            }
            QuickLongOp::BranchUnlessEq {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                let target = false_target.op_index()?;
                let native_index = builder.append(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::Equal,
                        lhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Slot(lhs),
                        ),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
            }
            QuickLongOp::BranchUnlessLe {
                lhs,
                rhs,
                false_target,
                next_target,
                resume_ip,
                ..
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                let target = false_target.op_index()?;
                let native_index = builder.append(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::LessThanOrEqual,
                        lhs: NativeStraightLongConditionOperand::Source(lhs),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    resume_ip,
                )?;
                pending_branches.push((native_index, target));
            }
            QuickLongOp::Jump { target } => {
                let native_index = builder.append(
                    NativeStraightLongOperation::Jump { target: 0 },
                    plan.target_ip(target)?,
                )?;
                pending_jumps.push((native_index, target.op_index()?));
            }
            QuickLongOp::ModConst {
                value,
                divisor,
                result,
                next_target,
                resume_ip,
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                builder.append(
                    NativeStraightLongOperation::Modulo {
                        value: QuickLongOperand::Slot(value),
                        divisor,
                        result,
                    },
                    resume_ip,
                )?;
            }
            QuickLongOp::Assign {
                destination,
                source,
                next_target,
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                builder.append(
                    NativeStraightLongOperation::Move {
                        source: QuickLongOperand::Slot(source),
                        result: destination,
                    },
                    plan.target_ip(next_target)?.saturating_sub(1),
                )?;
            }
            QuickLongOp::AssignLongLiteral {
                destination,
                value,
                next_target,
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                builder.append(
                    NativeStraightLongOperation::Move {
                        source: QuickLongOperand::Const(value),
                        result: destination,
                    },
                    plan.target_ip(next_target)?.saturating_sub(1),
                )?;
            }
            QuickLongOp::AssignStringLiteral {
                destination,
                literal,
                next_target,
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                builder.append(
                    NativeStraightLongOperation::StringToken {
                        token: builder.token_for_literal(literal)?,
                        result: destination,
                    },
                    plan.target_ip(next_target)?.saturating_sub(1),
                )?;
            }
            QuickLongOp::AssignStringSlot {
                destination,
                source,
                next_target,
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                builder.append(
                    NativeStraightLongOperation::Move {
                        source: QuickLongOperand::Slot(source),
                        result: destination,
                    },
                    plan.target_ip(next_target)?.saturating_sub(1),
                )?;
            }
            QuickLongOp::VirtualObjectArrayPipeline {
                constructor_arguments,
                consumers,
                consumer_count,
                trailing_key_literal,
                trailing_result,
                next_target,
                resume_ip,
                ..
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                let QuickResolvedObjectOp::VirtualPipeline { pipeline } =
                    *resolved_object_ops.get(plan_index)?
                else {
                    return None;
                };
                builder.lower_virtual_pipeline(
                    op_array,
                    &pipeline,
                    &constructor_arguments,
                    &consumers,
                    consumer_count,
                    trailing_key_literal,
                    trailing_result,
                    next_target,
                    resume_ip,
                )?;
                has_virtual_pipeline = true;
                has_typed_method = true;
            }
            QuickLongOp::PropertyMethodCall { call } => {
                if call.next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                let QuickResolvedObjectOp::PropertyMethod {
                    receiver,
                    target,
                    plan: property_plan,
                    property_slots,
                    property_count,
                } = *resolved_object_ops.get(plan_index)?
                else {
                    return None;
                };
                builder.lower_property_method(
                    plan_index,
                    receiver,
                    target,
                    &*property_plan,
                    &property_slots,
                    property_count,
                    &call,
                )?;
                has_property_method = true;
                has_typed_method = true;
            }
            QuickLongOp::ScalarMethodCall { call, result } => {
                if call.next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                match *resolved_object_ops.get(plan_index)? {
                    QuickResolvedObjectOp::ScalarMethod {
                        target,
                        plan: scalar_plan,
                    } => builder.lower_scalar_method(target, &*scalar_plan, &call, result)?,
                    QuickResolvedObjectOp::ObjectLongMethod {
                        target,
                        user,
                        plan: object_plan,
                        ..
                    } => {
                        let mixed_call = QuickObjectLongMethodCall {
                            guard: call.guard,
                            arguments: call.arguments.map(QuickObjectLongArgument::Long),
                            argument_count: call.argument_count,
                            next_target: call.next_target,
                            resume_ip: call.resume_ip,
                        };
                        builder.lower_object_method(
                            op_array,
                            target,
                            &*user,
                            &*object_plan,
                            &mixed_call,
                            result,
                        )?;
                    }
                    _ => return None,
                }
                has_typed_method = true;
            }
            QuickLongOp::ObjectLongMethodCall { call, result } => {
                if call.next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                match *resolved_object_ops.get(plan_index)? {
                    QuickResolvedObjectOp::ComposedTypedMethod {
                        target,
                        plan: typed_plan,
                    } => builder.lower_typed_method(target, &*typed_plan, &call, result)?,
                    QuickResolvedObjectOp::ObjectLongMethod {
                        target,
                        user,
                        plan: object_plan,
                        ..
                    } => builder.lower_object_method(
                        op_array,
                        target,
                        &*user,
                        &*object_plan,
                        &call,
                        result,
                    )?,
                    _ => return None,
                }
                has_typed_method = true;
            }
            QuickLongOp::FetchArrayLong {
                array,
                index: QuickArrayIndex::ValueSlot(key),
                result,
                destination,
                resume_ip,
                ..
            } => {
                let fusion = plan.array_update_fusions.get(plan_index).copied().flatten()?;
                if plan_index + 2 >= body_end
                    || fusion.next_target.op_index() != Some(plan_index + 3)
                    || builder.context_count + string_token_count
                        > NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES
                {
                    return None;
                }
                let entry_base = builder.context_count as u8;
                for token in 0..string_token_count {
                    builder.context_array_slots[builder.context_count] = array;
                    builder.context_tokens[builder.context_count] = token as u8;
                    builder.context_count += 1;
                }
                plan_to_native[plan_index] = builder.operation_count as u8;
                builder.append(
                    NativeStraightLongOperation::HashLoad {
                        key,
                        entry_base,
                        token_count: string_token_count as u8,
                        result,
                        destination,
                    },
                    resume_ip,
                )?;
                plan_to_native[plan_index + 1] = builder.operation_count as u8;
                builder.append(
                    NativeStraightLongOperation::Binary {
                        kind: fusion.kind,
                        lhs: fusion.lhs,
                        rhs: fusion.rhs,
                        result: fusion.result,
                    },
                    fusion.arithmetic_resume_ip,
                )?;
                let QuickLongOp::StoreArrayLong {
                    resume_ip: store_resume_ip,
                    ..
                } = plan.ops[plan_index + 2]
                else {
                    return None;
                };
                plan_to_native[plan_index + 2] = builder.operation_count as u8;
                builder.append(
                    NativeStraightLongOperation::HashStore {
                        key,
                        entry_base,
                        token_count: string_token_count as u8,
                        source: QuickLongOperand::Slot(fusion.result),
                    },
                    store_resume_ip,
                )?;
                has_hash_update = true;
                plan_index += 2;
            }
            QuickLongOp::Binary {
                kind,
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                builder.append(
                    NativeStraightLongOperation::Binary {
                        kind,
                        lhs,
                        rhs,
                        result,
                    },
                    resume_ip,
                )?;
            }
            QuickLongOp::BinaryAssign {
                kind,
                lhs,
                rhs,
                result,
                destination,
                next_target,
                resume_ip,
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                builder.append(
                    NativeStraightLongOperation::BinaryAssign {
                        kind,
                        lhs,
                        rhs,
                        result,
                        destination,
                    },
                    resume_ip,
                )?;
            }
            QuickLongOp::Add {
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } => {
                if next_target.op_index() != Some(plan_index + 1) {
                    return None;
                }
                builder.append(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(lhs),
                        rhs: QuickLongOperand::Slot(rhs),
                        result,
                    },
                    resume_ip,
                )?;
            }
            QuickLongOp::TraceGuard {
                kind,
                lhs,
                rhs,
                expected,
                condition_tmp: Some(condition_tmp),
                next_target,
                resume_ip,
            } => {
                if next_target.op_index() != Some(plan_index + 1)
                    || trace_guard_count == NATIVE_STRAIGHT_LONG_MAX_OPERATIONS
                {
                    return None;
                }
                let operation_index = builder.append(
                    NativeStraightLongOperation::Guard {
                        kind,
                        lhs: NativeStraightLongConditionOperand::Source(lhs),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        expected,
                    },
                    resume_ip,
                )?;
                trace_guard_operation_indices[trace_guard_count] = operation_index;
                trace_guard_condition_slots[trace_guard_count] = u8::try_from(condition_tmp).ok()?;
                trace_guard_expected[trace_guard_count] = expected;
                trace_guard_count += 1;
            }
            _ => return None,
        }
        plan_index += 1;
    }
    plan_to_native[body_end] = u8::try_from(builder.operation_count).ok()?;
    for (native_index, target_plan) in pending_branches {
        let false_target = *plan_to_native.get(target_plan)?;
        if false_target == u8::MAX {
            return None;
        }
        let NativeStraightLongOperation::BranchUnless { kind, lhs, rhs, .. } =
            builder.operations[native_index as usize]
        else {
            return None;
        };
        builder.operations[native_index as usize] = NativeStraightLongOperation::BranchUnless {
            kind,
            lhs,
            rhs,
            false_target,
        };
    }
    for (native_index, target_plan) in pending_jumps {
        let target = *plan_to_native.get(target_plan)?;
        if target == u8::MAX {
            return None;
        }
        builder.operations[native_index as usize] = NativeStraightLongOperation::Jump { target };
    }
    if (!has_hash_update && !has_virtual_pipeline && !has_property_method)
        || !has_typed_method
        || builder.operation_count == 0
    {
        return None;
    }

    let config = NativeStraightLongLoopConfig {
        induction_slot: post_value,
        bound: header_rhs,
        operations: builder.operations,
        operation_count: builder.operation_count as u8,
        post_result,
    };
    let mut mutable_mask = config
        .operations
        .iter()
        .copied()
        .take(config.operation_count as usize)
        .fold(1u64 << post_value, |mask, operation| {
            mask | operation.shadow_output_mask()
        });
    if let Some(slot) = post_result {
        mutable_mask |= 1u64 << slot;
    }
    if matches!(header_rhs, QuickLongOperand::Slot(slot) if mutable_mask & (1u64 << slot) != 0) {
        return None;
    }
    let mut mutable_slots = [0u8; NATIVE_QUICK_LONG_SLOT_CAPACITY];
    let mut mutable_slot_count = 0usize;
    while mutable_mask != 0 {
        if mutable_slot_count == mutable_slots.len() {
            return None;
        }
        let slot = mutable_mask.trailing_zeros() as u8;
        mutable_mask &= mutable_mask - 1;
        mutable_slots[mutable_slot_count] = slot;
        mutable_slot_count += 1;
    }

    Some(NativeQuickLongMixedKernel {
        config,
        header_condition_tmp,
        body_target,
        exit_target,
        post_resume_ip,
        operation_resume_ips: builder.operation_resume_ips,
        string_literals: builder.string_literals,
        string_token_count: builder.string_token_count as u8,
        context_array_slots: builder.context_array_slots,
        context_tokens: builder.context_tokens,
        context_count: builder.context_count as u8,
        property_binding_op_indices: builder.property_binding_op_indices,
        property_binding_property_indices: builder.property_binding_property_indices,
        property_binding_slots: builder.property_binding_slots,
        property_binding_count: builder.property_binding_count as u8,
        call_targets: builder.call_targets,
        call_completion_operations: builder.call_completion_operations,
        call_count: builder.call_count as u8,
        trace_guard_operation_indices,
        trace_guard_condition_slots,
        trace_guard_expected,
        trace_guard_count: trace_guard_count as u8,
        long_output_mask: plan.long_output_mask,
        string_output_mask: plan.string_output_mask,
        mutable_slots,
        mutable_slot_count: mutable_slot_count as u8,
    })
}
