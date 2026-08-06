/// Recognize a closed `Long induction + Double scalar leaf + Double
/// accumulation` loop. Caller argument expressions are retained as a small
/// target-neutral Double program; exact-Double CVs are guarded at entry and
/// the Long induction value is converted by the selected execution tier.
pub fn detect_double_call_accumulate_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickDoubleCallAccumulateLoop> {
    if header_ip.checked_add(8)? > backedge_ip || backedge_ip >= op_array.instructions.len() {
        return None;
    }
    let condition = *op_array.instructions.get(header_ip)?;
    let branch = *op_array.instructions.get(header_ip + 1)?;
    let backedge = *op_array.instructions.get(backedge_ip)?;
    let (induction_cv, bound, condition_tmp, exit_ip) = match condition.opcode {
        OpCode::IsSmaller
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Cv
                && condition.result_type == OpType::Tmp
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op1 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Cv(condition.op2),
                Some(condition.result),
                branch.op2 as usize,
            )
        }
        OpCode::IsSmaller_CvConst
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Const
                && condition.result_type == OpType::Tmp
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op1 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Const(long_literal(op_array, condition.op2)?),
                Some(condition.result),
                branch.op2 as usize,
            )
        }
        OpCode::JmpZ_Lt_CvConst
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Const
                && condition.result_type == OpType::Unused
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op2_type == OpType::Unused
                && branch.op2 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Const(long_literal(op_array, condition.op2)?),
                None,
                condition.result as usize,
            )
        }
        _ => return None,
    };
    if exit_ip <= backedge_ip || exit_ip >= op_array.instructions.len() {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if total_slots > 64 {
        return None;
    }
    let region = &op_array.instructions[header_ip..=backedge_ip];
    let mut initializer_ip = header_ip + 2;
    let mut typed_invariant_source = None;
    let mut json_paths: Vec<Option<Vec<QuickInvariantPathElement>>> =
        vec![None; total_slots as usize];
    let mut json_fetch_mask = 0u64;
    let mut json_parent_mask = 0u64;
    let possible_producer = *op_array.instructions.get(initializer_ip)?;
    if possible_producer.opcode == OpCode::DirectInternalCall2
        && crate::builtin_metadata::DirectInternalKind::from_id(possible_producer.extended_value)
            == Some(crate::builtin_metadata::DirectInternalKind::JsonDecode)
    {
        let source = detect_json_typed_invariant_source(op_array, region, initializer_ip)?;
        json_paths
            .get_mut(source.destination as usize)?
            .replace(Vec::new());
        typed_invariant_source = Some(source);
        initializer_ip += 2;
    }
    let initializer = *op_array.instructions.get(initializer_ip)?;
    let (argument_count, argument_offset, guard, receiver_slot) = match initializer.opcode {
        OpCode::InitFcall if initializer.op1 <= 8 => (
            initializer.op1 as usize,
            0usize,
            ScalarLongCallGuard::FunctionCache {
                cache_ip: u32::try_from(initializer_ip).ok()?,
            },
            None,
        ),
        OpCode::InitMethodCall
            if initializer.op1_type == OpType::Cv && initializer.extended_value <= 8 =>
        {
            (
                initializer.extended_value as usize,
                1usize,
                ScalarLongCallGuard::MethodCache {
                    cache_ip: u32::try_from(initializer_ip).ok()?,
                    receiver_slot: initializer.op1,
                },
                Some(initializer.op1),
            )
        }
        _ => return None,
    };
    let mut outputs = [QuickDoubleSource::Constant(0.0); 8];
    let mut operations = Vec::with_capacity(8);
    let mut produced_temporary_slots = [u16::MAX; 8];
    let mut expression_count = 0usize;
    let mut sent_arguments = 0usize;
    let mut input_slots = [u16::MAX; 8];
    let mut input_count = 0usize;
    let mut double_input_mask = 0u64;
    let mut cursor = initializer_ip + 1;
    while sent_arguments < argument_count {
        let send = *op_array.instructions.get(cursor)?;
        if send.opcode == OpCode::FetchDimR
            && matches!(send.op1_type, OpType::Cv | OpType::Tmp | OpType::Var)
            && send.result_type == OpType::Tmp
        {
            let Some(mut path) = json_paths
                .get(send.op1 as usize)
                .and_then(|path| path.as_ref())
                .cloned()
            else {
                return None;
            };
            let element = fixed_invariant_path_element(op_array, send.op2_type, send.op2)?;
            if path.len() == 8 {
                return None;
            }
            path.push(element);
            json_paths.get_mut(send.result as usize)?.replace(path);
            add_mask_slot(&mut json_fetch_mask, send.result, total_slots)?;
            add_mask_slot(&mut json_parent_mask, send.op1, total_slots)?;
            cursor += 1;
            continue;
        }
        if matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            if send.op2 as usize != sent_arguments + argument_offset {
                return None;
            }
            let output = if matches!(send.op1_type, OpType::Tmp | OpType::Var)
                && json_paths
                    .get(send.op1 as usize)
                    .and_then(|path| path.as_ref())
                    .is_some()
            {
                let path = json_paths.get(send.op1 as usize)?.as_ref()?.clone();
                if path.is_empty() {
                    return None;
                }
                add_mask_slot(&mut double_input_mask, send.op1, total_slots)?;
                let input = if let Some(index) = input_slots[..input_count]
                    .iter()
                    .position(|slot| *slot == send.op1)
                {
                    index
                } else {
                    if input_count == input_slots.len() {
                        return None;
                    }
                    let index = input_count;
                    input_slots[index] = send.op1;
                    input_count += 1;
                    index
                };
                let source = typed_invariant_source.as_mut()?;
                if source.double_output_mask & (1u64 << send.op1) == 0 {
                    source.double_output_mask |= 1u64 << send.op1;
                    source.projections.push(QuickTypedInvariantProjection {
                        path: path.into_boxed_slice(),
                        result: send.op1,
                        kind: QuickInvariantValueKind::Double,
                    });
                }
                ProvenQuickDoubleSource {
                    source: QuickDoubleSource::Input(input as u8),
                    is_double: true,
                }
            } else {
                quick_double_argument_source(
                    op_array,
                    send.op1_type,
                    send.op1,
                    induction_cv,
                    &produced_temporary_slots,
                    expression_count,
                    &mut input_slots,
                    &mut input_count,
                    &mut double_input_mask,
                    total_slots,
                )?
            };
            if !output.is_double {
                return None;
            }
            outputs[sent_arguments] = output.source;
            sent_arguments += 1;
            cursor += 1;
            continue;
        }
        let kind = match send.opcode {
            OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp => {
                crate::vm::function::ScalarDoubleOpKind::Add
            }
            OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                crate::vm::function::ScalarDoubleOpKind::Subtract
            }
            OpCode::Mul => crate::vm::function::ScalarDoubleOpKind::Multiply,
            OpCode::Div => crate::vm::function::ScalarDoubleOpKind::Divide,
            _ => return None,
        };
        if expression_count == 8
            || !matches!(send.result_type, OpType::Tmp | OpType::Var)
            || produced_temporary_slots[..expression_count].contains(&send.result)
        {
            return None;
        }
        let lhs = quick_double_argument_source(
            op_array,
            send.op1_type,
            send.op1,
            induction_cv,
            &produced_temporary_slots,
            expression_count,
            &mut input_slots,
            &mut input_count,
            &mut double_input_mask,
            total_slots,
        )?;
        let rhs = quick_double_argument_source(
            op_array,
            send.op2_type,
            send.op2,
            induction_cv,
            &produced_temporary_slots,
            expression_count,
            &mut input_slots,
            &mut input_count,
            &mut double_input_mask,
            total_slots,
        )?;
        if !lhs.is_double && !rhs.is_double {
            return None;
        }
        operations.push(QuickDoubleArgumentOp {
            kind,
            lhs: lhs.source,
            rhs: rhs.source,
        });
        produced_temporary_slots[expression_count] = send.result;
        expression_count += 1;
        cursor += 1;
    }

    let do_fcall_ip = cursor;
    let do_fcall = *op_array.instructions.get(do_fcall_ip)?;
    let sum_ip = do_fcall_ip + 1;
    let sum = *op_array.instructions.get(sum_ip)?;
    let assign = *op_array.instructions.get(sum_ip + 1)?;
    let increment_ip = sum_ip + 2;
    let increment = *op_array.instructions.get(increment_ip)?;
    if increment_ip + 1 != backedge_ip
        || do_fcall.opcode != OpCode::DoFcall
        || do_fcall.result_type != OpType::Tmp
        || sum.opcode != OpCode::Add_CvTmp
        || sum.op1_type != OpType::Cv
        || sum.op2_type != OpType::Tmp
        || sum.op2 != do_fcall.result
        || sum.result_type != OpType::Tmp
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op1 != sum.op1
        || assign.op2_type != OpType::Tmp
        || assign.op2 != sum.result
        || increment.op1_type != OpType::Cv
        || increment.op1 != induction_cv
        || !matches!(increment.opcode, OpCode::PreInc | OpCode::PostInc)
        || !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
    {
        return None;
    }
    let accumulator_cv = sum.op1;
    let increment_kind = if increment.opcode == OpCode::PreInc {
        QuickIncrementKind::Pre
    } else {
        QuickIncrementKind::Post
    };
    let increment_tmp = match increment.result_type {
        OpType::Unused => None,
        OpType::Tmp => Some(increment.result),
        _ => return None,
    };
    if accumulator_cv == induction_cv
        || matches!(bound, QuickLongBound::Cv(slot) if slot == induction_cv || slot == accumulator_cv)
        || double_input_mask & ((1u64 << induction_cv) | (1u64 << accumulator_cv)) != 0
        || matches!(bound, QuickLongBound::Cv(slot) if double_input_mask & (1u64 << slot) != 0)
        || receiver_slot.is_some_and(|slot| {
            slot == induction_cv
                || slot == accumulator_cv
                || slot as u32 >= op_array.num_cvs
                || double_input_mask & (1u64 << slot) != 0
                || matches!(bound, QuickLongBound::Cv(bound) if bound == slot)
        })
        || induction_cv as u32 >= op_array.num_cvs
        || accumulator_cv as u32 >= op_array.num_cvs
        || matches!(bound, QuickLongBound::Cv(slot) if slot as u32 >= op_array.num_cvs)
    {
        return None;
    }
    if let Some(source) = typed_invariant_source.as_ref() {
        let input_slot = match source.producer {
            QuickTypedInvariantProducer::JsonDecodeAssociative {
                input: QuickInvariantInput::StringSlot(slot),
            } => Some(slot),
            QuickTypedInvariantProducer::JsonDecodeAssociative {
                input: QuickInvariantInput::StringLiteral(_),
            } => None,
        };
        if source.projections.is_empty()
            || json_fetch_mask & !(json_parent_mask | source.double_output_mask) != 0
            || source.destination == induction_cv
            || source.destination == accumulator_cv
            || matches!(bound, QuickLongBound::Cv(slot) if source.destination == slot)
            || receiver_slot == Some(source.destination)
            || input_slot.is_some_and(|slot| {
                slot == induction_cv
                    || slot == accumulator_cv
                    || matches!(bound, QuickLongBound::Cv(bound) if bound == slot)
                    || receiver_slot == Some(slot)
                    || double_input_mask & (1u64 << slot) != 0
            })
        {
            return None;
        }
    }
    let mut temporary_mask = 0u64;
    for slot in [
        condition_tmp,
        Some(do_fcall.result),
        Some(sum.result),
        increment_tmp,
    ]
    .into_iter()
    .flatten()
    {
        add_mask_slot(&mut temporary_mask, slot, total_slots)?;
    }
    for slot in produced_temporary_slots
        .iter()
        .copied()
        .take(expression_count)
    {
        add_mask_slot(&mut temporary_mask, slot, total_slots)?;
    }
    let mut json_temporaries = json_fetch_mask;
    while json_temporaries != 0 {
        let slot = json_temporaries.trailing_zeros() as u16;
        json_temporaries &= json_temporaries - 1;
        add_mask_slot(&mut temporary_mask, slot, total_slots)?;
    }
    let expected_temporary_count = usize::from(condition_tmp.is_some())
        + 2
        + usize::from(increment_tmp.is_some())
        + expression_count
        + json_fetch_mask.count_ones() as usize;
    if temporary_mask.count_ones() as usize != expected_temporary_count {
        return None;
    }

    Some(QuickDoubleCallAccumulateLoop {
        header_ip,
        exit_ip,
        induction_cv,
        bound,
        accumulator_cv,
        condition_tmp,
        guard,
        argument_program: QuickDoubleArgumentProgram {
            operations: operations.into_boxed_slice(),
            outputs,
            output_count: argument_count as u8,
            input_slots,
            input_count: input_count as u8,
        },
        typed_invariant_source,
        term_tmp: do_fcall.result,
        sum_tmp: sum.result,
        increment_kind,
        increment_tmp,
        sum_ip,
        increment_ip,
        #[cfg(all(
            feature = "jit-prototype",
            any(
                all(target_arch = "aarch64", target_os = "macos"),
                all(target_arch = "x86_64", target_os = "linux")
            )
        ))]
        native_jit: crate::jit::QuickDoubleCallAccumulateJitCache::new(),
    })
}

fn quick_scalar_long_source(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
    produced_temporary_slots: &[u16; 8],
    produced_temporary_count: usize,
) -> Option<ScalarLongSource> {
    match op_type {
        OpType::Cv => Some(ScalarLongSource::Input(operand)),
        OpType::Const => long_literal(op_array, operand).map(ScalarLongSource::Constant),
        OpType::Tmp | OpType::Var => produced_temporary_slots[..produced_temporary_count]
            .iter()
            .position(|slot| *slot == operand)
            .and_then(|index| u8::try_from(index).ok())
            .map(ScalarLongSource::Temporary),
        OpType::Unused => None,
    }
}
