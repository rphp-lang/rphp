fn detect_scalar_call_tree(
    op_array: &OpArray,
    initializer_ip: usize,
    total_slots: u32,
    long_input_mask: &mut u64,
    object_input_mask: &mut u64,
    depth: usize,
) -> Option<usize> {
    if depth >= 8 {
        return None;
    }
    let initializer = *op_array.instructions.get(initializer_ip)?;
    let (argument_count, argument_offset) = match initializer.opcode {
        OpCode::InitFcall => (initializer.op1 as usize, 0usize),
        OpCode::InitMethodCall if initializer.op1_type == OpType::Cv => {
            add_mask_slot(object_input_mask, initializer.op1, total_slots)?;
            (initializer.extended_value as usize, 1usize)
        }
        _ => return None,
    };
    if argument_count > 8 {
        return None;
    }

    let mut cursor = initializer_ip + 1;
    for argument_index in 0..argument_count {
        let instruction = *op_array.instructions.get(cursor)?;
        let destination = u16::try_from(argument_index.checked_add(argument_offset)?).ok()?;

        if matches!(instruction.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            if instruction.op2 != destination {
                return None;
            }
            match instruction.op1_type {
                OpType::Cv => {
                    add_mask_slot(long_input_mask, instruction.op1, total_slots)?;
                }
                OpType::Const => {
                    long_literal(op_array, instruction.op1)?;
                }
                _ => return None,
            }
            cursor += 1;
            continue;
        }

        if matches!(
            instruction.opcode,
            OpCode::Add
                | OpCode::Add_CvTmp
                | OpCode::Add_TmpTmp
                | OpCode::Sub
                | OpCode::Sub_CvConst
                | OpCode::Sub_TmpTmp
                | OpCode::Mul
        ) {
            if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                return None;
            }
            for (op_type, operand) in [
                (instruction.op1_type, instruction.op1),
                (instruction.op2_type, instruction.op2),
            ] {
                match op_type {
                    OpType::Cv => {
                        add_mask_slot(long_input_mask, operand, total_slots)?;
                    }
                    OpType::Const => {
                        long_literal(op_array, operand)?;
                    }
                    _ => return None,
                }
            }
            let send = *op_array.instructions.get(cursor + 1)?;
            if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
                || send.op1 != instruction.result
                || send.op2 != destination
            {
                return None;
            }
            cursor += 2;
            continue;
        }

        if matches!(
            instruction.opcode,
            OpCode::InitFcall | OpCode::InitMethodCall
        ) {
            let nested_do_fcall_ip = detect_scalar_call_tree(
                op_array,
                cursor,
                total_slots,
                long_input_mask,
                object_input_mask,
                depth + 1,
            )?;
            let nested_do_fcall = *op_array.instructions.get(nested_do_fcall_ip)?;
            let send = *op_array.instructions.get(nested_do_fcall_ip + 1)?;
            if !matches!(nested_do_fcall.result_type, OpType::Tmp | OpType::Var)
                || !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
                || send.op1 != nested_do_fcall.result
                || send.op2 != destination
            {
                return None;
            }
            cursor = nested_do_fcall_ip + 2;
            continue;
        }

        return None;
    }

    let do_fcall = *op_array.instructions.get(cursor)?;
    (do_fcall.opcode == OpCode::DoFcall).then_some(cursor)
}

/// Recognize one side-effect-free scalar loop region.
///
/// `header_ip` is the target of the backward `Jmp`; `backedge_ip` is the
/// instruction position of that `Jmp`.
pub fn detect_long_accumulate_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickLongAccumulateLoop> {
    if header_ip.checked_add(5)? > backedge_ip
        || header_ip.checked_add(23)? < backedge_ip
        || backedge_ip >= op_array.instructions.len()
    {
        return None;
    }

    let condition = op_array.instructions[header_ip];
    let branch = op_array.instructions[header_ip + 1];
    let backedge = op_array.instructions[backedge_ip];

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

    let first_body = op_array.instructions[header_ip + 2];
    let call_ip = header_ip + 2;
    let nested_function_call_shape = if first_body.opcode == OpCode::InitFcall {
        let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
        if total_slots > 64 {
            return None;
        }
        let mut long_input_mask = 0u64;
        let mut object_input_mask = 0u64;
        let do_fcall_ip = detect_scalar_call_tree(
            op_array,
            call_ip,
            total_slots,
            &mut long_input_mask,
            &mut object_input_mask,
            0,
        )?;
        let contains_nested_call =
            op_array.instructions[call_ip + 1..do_fcall_ip]
                .iter()
                .any(|instruction| {
                    matches!(
                        instruction.opcode,
                        OpCode::InitFcall | OpCode::InitMethodCall
                    )
                });
        if contains_nested_call {
            if long_input_mask & object_input_mask != 0 {
                return None;
            }
            let argument_count = u8::try_from(first_body.op1).ok()?;
            let do_fcall = op_array.instructions[do_fcall_ip];
            let sum_ip = do_fcall_ip + 1;
            let sum = *op_array.instructions.get(sum_ip)?;
            if backedge_ip < do_fcall_ip + 4
                || do_fcall.result_type != OpType::Tmp
                || sum.opcode != OpCode::Add_CvTmp
                || sum.op1_type != OpType::Cv
                || sum.op2_type != OpType::Tmp
                || sum.op2 != do_fcall.result
                || sum.result_type != OpType::Tmp
            {
                return None;
            }
            Some((
                sum.op1,
                QuickLongTerm::ScalarCallTree {
                    guard: ScalarLongCallGuard::FunctionCache {
                        cache_ip: u32::try_from(call_ip).ok()?,
                    },
                    do_fcall_ip,
                    long_input_mask,
                    object_input_mask,
                    argument_count,
                    term_tmp: do_fcall.result,
                },
                sum.result,
                sum_ip,
                sum_ip + 1,
            ))
        } else {
            None
        }
    } else {
        None
    };
    let scalar_call_shape = if nested_function_call_shape.is_some() {
        nested_function_call_shape
    } else if first_body.opcode == OpCode::InitFcall {
        let argument_count = first_body.op1 as usize;
        if argument_count > 8 {
            return None;
        }
        let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
        if total_slots > 64 {
            return None;
        }
        let mut long_input_mask = 0u64;
        let mut operations = Vec::with_capacity(8);
        let mut arguments = [ScalarLongSource::Constant(0); 8];
        let mut produced_temporary_slots = [u16::MAX; 8];
        let mut expression_count = 0usize;
        let mut sent_arguments = 0usize;
        let mut do_fcall_ip = header_ip + 3;
        while sent_arguments < argument_count {
            let instruction = *op_array.instructions.get(do_fcall_ip)?;
            match instruction.opcode {
                OpCode::Add
                | OpCode::Add_CvTmp
                | OpCode::Add_TmpTmp
                | OpCode::Sub
                | OpCode::Sub_CvConst
                | OpCode::Sub_TmpTmp
                | OpCode::Mul => {
                    if expression_count == 8
                        || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                    {
                        return None;
                    }
                    for (op_type, operand) in [
                        (instruction.op1_type, instruction.op1),
                        (instruction.op2_type, instruction.op2),
                    ] {
                        if op_type == OpType::Cv {
                            add_mask_slot(&mut long_input_mask, operand, total_slots)?;
                        }
                    }
                    let lhs = quick_scalar_long_source(
                        op_array,
                        instruction.op1_type,
                        instruction.op1,
                        &produced_temporary_slots,
                        expression_count,
                    )?;
                    let rhs = quick_scalar_long_source(
                        op_array,
                        instruction.op2_type,
                        instruction.op2,
                        &produced_temporary_slots,
                        expression_count,
                    )?;
                    if produced_temporary_slots[..expression_count].contains(&instruction.result) {
                        return None;
                    }
                    let kind = match instruction.opcode {
                        OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp => {
                            ScalarLongOpKind::Add
                        }
                        OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                            ScalarLongOpKind::Subtract
                        }
                        OpCode::Mul => ScalarLongOpKind::Multiply,
                        _ => unreachable!(),
                    };
                    operations.push(ScalarLongOp { kind, lhs, rhs });
                    produced_temporary_slots[expression_count] = instruction.result;
                    expression_count += 1;
                }
                OpCode::SendVal if instruction.op2 as usize == sent_arguments => {
                    arguments[sent_arguments] = quick_scalar_long_source(
                        op_array,
                        instruction.op1_type,
                        instruction.op1,
                        &produced_temporary_slots,
                        expression_count,
                    )?;
                    sent_arguments += 1;
                }
                _ => return None,
            }
            do_fcall_ip += 1;
        }

        let do_fcall = op_array.instructions[do_fcall_ip];
        let sum_ip = do_fcall_ip + 1;
        let sum = op_array.instructions[sum_ip];
        if backedge_ip < do_fcall_ip + 4
            || do_fcall.opcode != OpCode::DoFcall
            || do_fcall.result_type != OpType::Tmp
            || sum.opcode != OpCode::Add_CvTmp
            || sum.op1_type != OpType::Cv
            || sum.op2_type != OpType::Tmp
            || sum.op2 != do_fcall.result
            || sum.result_type != OpType::Tmp
        {
            return None;
        }
        Some((
            sum.op1,
            QuickLongTerm::ScalarFunctionCall {
                guard: ScalarLongCallGuard::FunctionCache {
                    cache_ip: u32::try_from(header_ip + 2).ok()?,
                },
                do_fcall_ip,
                long_input_mask,
                argument_plan: Box::new(ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: arguments,
                    output_count: argument_count as u8,
                }),
                argument_count: argument_count as u8,
                term_tmp: do_fcall.result,
            },
            sum.result,
            sum_ip,
            sum_ip + 1,
        ))
    } else if first_body.opcode == OpCode::InitMethodCall {
        let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
        if total_slots > 64 {
            return None;
        }
        let mut long_input_mask = 0u64;
        let mut object_input_mask = 0u64;
        let call_ip = header_ip + 2;
        let do_fcall_ip = detect_scalar_call_tree(
            op_array,
            call_ip,
            total_slots,
            &mut long_input_mask,
            &mut object_input_mask,
            0,
        )?;
        if long_input_mask & object_input_mask != 0 {
            return None;
        }
        let argument_count = u8::try_from(first_body.extended_value).ok()?;
        let do_fcall = op_array.instructions[do_fcall_ip];
        let sum_ip = do_fcall_ip + 1;
        let sum = *op_array.instructions.get(sum_ip)?;
        if backedge_ip < do_fcall_ip + 4
            || do_fcall.result_type != OpType::Tmp
            || sum.opcode != OpCode::Add_CvTmp
            || sum.op1_type != OpType::Cv
            || sum.op2_type != OpType::Tmp
            || sum.op2 != do_fcall.result
            || sum.result_type != OpType::Tmp
        {
            return None;
        }
        Some((
            sum.op1,
            QuickLongTerm::ScalarCallTree {
                guard: ScalarLongCallGuard::MethodCache {
                    cache_ip: u32::try_from(call_ip).ok()?,
                    receiver_slot: first_body.op1,
                },
                do_fcall_ip,
                long_input_mask,
                object_input_mask,
                argument_count,
                term_tmp: do_fcall.result,
            },
            sum.result,
            sum_ip,
            sum_ip + 1,
        ))
    } else {
        None
    };
    let (accumulator_cv, term, sum_tmp, sum_ip, assign_ip) = if let Some(shape) = scalar_call_shape
    {
        shape
    } else if first_body.opcode == OpCode::Add
        && first_body.op1_type == OpType::Cv
        && first_body.op2_type == OpType::Cv
        && first_body.op2 == induction_cv
        && first_body.result_type == OpType::Tmp
        && op_array
            .instructions
            .get(header_ip + 3)
            .is_some_and(|assign| {
                assign.opcode == OpCode::AssignCv
                    && assign.op1_type == OpType::Cv
                    && assign.op1 == first_body.op1
                    && assign.op2_type == OpType::Tmp
                    && assign.op2 == first_body.result
                    && assign.result_type == OpType::Unused
            })
    {
        if backedge_ip < header_ip + 5 {
            return None;
        }
        (
            first_body.op1,
            QuickLongTerm::Induction,
            first_body.result,
            header_ip + 2,
            header_ip + 3,
        )
    } else if backedge_ip >= header_ip + 6
        && op_array.instructions.get(header_ip + 3).is_some_and(|sum| {
            sum.opcode == OpCode::Add_CvTmp
                && sum.op1_type == OpType::Cv
                && sum.op2_type == OpType::Tmp
                && sum.op2 == first_body.result
                && sum.result_type == OpType::Tmp
        })
    {
        let sum = op_array.instructions[header_ip + 3];
        if first_body.result_type != OpType::Tmp {
            return None;
        }
        let term = match (first_body.opcode, first_body.op1_type, first_body.op2_type) {
            (OpCode::Add, OpType::Cv, OpType::Const) if first_body.op1 == induction_cv => {
                QuickLongTerm::InductionPlusConst {
                    addend: long_literal(op_array, first_body.op2)?,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::Add, OpType::Const, OpType::Cv) if first_body.op2 == induction_cv => {
                QuickLongTerm::InductionPlusConst {
                    addend: long_literal(op_array, first_body.op1)?,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::Add, OpType::Cv, OpType::Cv)
                if first_body.op1 == induction_cv && first_body.op2 != induction_cv =>
            {
                QuickLongTerm::InductionPlusCv {
                    addend_cv: first_body.op2,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::Add, OpType::Cv, OpType::Cv)
                if first_body.op2 == induction_cv && first_body.op1 != induction_cv =>
            {
                QuickLongTerm::InductionPlusCv {
                    addend_cv: first_body.op1,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::FetchDimR, OpType::Cv, OpType::Cv) => QuickLongTerm::ArrayIndex {
                array_cv: first_body.op1,
                index: if first_body.op2 == induction_cv {
                    QuickArrayIndex::Long(QuickLongOperand::Slot(induction_cv))
                } else {
                    QuickArrayIndex::ValueSlot(first_body.op2)
                },
                term_tmp: first_body.result,
                destination: None,
                fetch_ip: header_ip + 2,
            },
            (OpCode::FetchDimR, OpType::Cv, OpType::Const) => QuickLongTerm::ArrayIndex {
                array_cv: first_body.op1,
                index: array_literal_index(op_array, first_body.op2)?,
                term_tmp: first_body.result,
                destination: None,
                fetch_ip: header_ip + 2,
            },
            (OpCode::Strlen_Cv, OpType::Cv, OpType::Unused) => QuickLongTerm::StringLength {
                string_cv: first_body.op1,
                term_tmp: first_body.result,
                term_ip: header_ip + 2,
            },
            (OpCode::DirectInternalCall1, OpType::Cv, OpType::Unused)
                if crate::builtin_metadata::DirectInternalKind::from_id(
                    first_body.extended_value,
                ) == Some(crate::builtin_metadata::DirectInternalKind::Abs) =>
            {
                QuickLongTerm::AbsLong {
                    operand_cv: first_body.op1,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            _ => return None,
        };
        (sum.op1, term, sum.result, header_ip + 3, header_ip + 4)
    } else {
        let materialize = op_array.instructions[header_ip + 3];
        let sum = op_array.instructions[header_ip + 4];
        if first_body.opcode != OpCode::FetchDimR
            || first_body.op1_type != OpType::Cv
            || !matches!(first_body.op2_type, OpType::Cv | OpType::Const)
            || first_body.result_type != OpType::Tmp
            || materialize.opcode != OpCode::AssignCv
            || materialize.op1_type != OpType::Cv
            || materialize.op2_type != OpType::Tmp
            || materialize.op2 != first_body.result
            || materialize.result_type != OpType::Unused
            || sum.opcode != OpCode::Add
            || sum.op1_type != OpType::Cv
            || sum.op2_type != OpType::Cv
            || sum.result_type != OpType::Tmp
        {
            return None;
        }
        let accumulator_cv = if sum.op1 == materialize.op1 && sum.op2 != materialize.op1 {
            sum.op2
        } else if sum.op2 == materialize.op1 && sum.op1 != materialize.op1 {
            sum.op1
        } else {
            return None;
        };
        (
            accumulator_cv,
            QuickLongTerm::ArrayIndex {
                array_cv: first_body.op1,
                index: match first_body.op2_type {
                    OpType::Cv => QuickArrayIndex::ValueSlot(first_body.op2),
                    OpType::Const => array_literal_index(op_array, first_body.op2)?,
                    _ => unreachable!(),
                },
                term_tmp: first_body.result,
                destination: Some(materialize.op1),
                fetch_ip: header_ip + 2,
            },
            sum.result,
            header_ip + 4,
            header_ip + 5,
        )
    };

    let assign = op_array.instructions[assign_ip];
    if assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op1 != accumulator_cv
        || assign.op2_type != OpType::Tmp
        || assign.op2 != sum_tmp
        || assign.result_type != OpType::Unused
    {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    let direct_increment_ip = assign_ip + 1;
    let (tail_guard, increment_ip) = if matches!(
        op_array.instructions.get(direct_increment_ip)?.opcode,
        OpCode::PreInc | OpCode::PostInc
    ) {
        (None, direct_increment_ip)
    } else {
        let increment_ip = backedge_ip.checked_sub(1)?;
        (
            Some(detect_long_tail_trace_guard(
                op_array,
                direct_increment_ip,
                increment_ip,
                total_slots,
            )?),
            increment_ip,
        )
    };
    let increment = op_array.instructions[increment_ip];
    let increment_kind = match increment.opcode {
        OpCode::PreInc => QuickIncrementKind::Pre,
        OpCode::PostInc => QuickIncrementKind::Post,
        _ => return None,
    };
    if increment.op1_type != OpType::Cv || increment.op1 != induction_cv {
        return None;
    }
    let increment_tmp = match increment.result_type {
        OpType::Unused => None,
        OpType::Tmp => Some(increment.result),
        _ => return None,
    };

    if !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
    {
        return None;
    }

    if accumulator_cv == induction_cv
        || matches!(bound, QuickLongBound::Cv(cv) if cv == induction_cv || cv == accumulator_cv)
        || matches!(
            term,
            QuickLongTerm::InductionPlusCv { addend_cv, .. }
                if addend_cv == induction_cv || addend_cv == accumulator_cv
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex { array_cv, .. }
                if array_cv == induction_cv || array_cv == accumulator_cv
        )
        || matches!(
            term,
            QuickLongTerm::StringLength { string_cv, .. }
                if string_cv == induction_cv || string_cv == accumulator_cv
        )
        || matches!(
            term,
            QuickLongTerm::AbsLong { operand_cv, .. }
                if operand_cv == accumulator_cv
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex {
                array_cv,
                index: QuickArrayIndex::ValueSlot(index_cv),
                destination,
                ..
            } if index_cv == induction_cv
                || index_cv == accumulator_cv
                || index_cv == array_cv
                || destination == Some(index_cv)
                || matches!(bound, QuickLongBound::Cv(bound_cv) if index_cv == bound_cv)
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex {
                array_cv,
                destination: Some(destination),
                ..
            } if destination == induction_cv
                || destination == accumulator_cv
                || destination == array_cv
                || matches!(bound, QuickLongBound::Cv(bound_cv) if destination == bound_cv)
        )
    {
        return None;
    }

    let mut temporary_slots = vec![sum_tmp];
    if let Some(slot) = condition_tmp {
        temporary_slots.push(slot);
    }
    match term {
        QuickLongTerm::Induction => {}
        QuickLongTerm::InductionPlusConst { term_tmp, .. }
        | QuickLongTerm::InductionPlusCv { term_tmp, .. }
        | QuickLongTerm::ArrayIndex { term_tmp, .. }
        | QuickLongTerm::StringLength { term_tmp, .. }
        | QuickLongTerm::AbsLong { term_tmp, .. }
        | QuickLongTerm::ScalarFunctionCall { term_tmp, .. }
        | QuickLongTerm::ScalarCallTree { term_tmp, .. } => {
            temporary_slots.push(term_tmp);
        }
    }
    if let Some(slot) = increment_tmp {
        temporary_slots.push(slot);
    }
    if let Some(slot) = tail_guard.and_then(|guard| guard.condition_tmp) {
        temporary_slots.push(slot);
    }
    let temporary_slot_count = temporary_slots.len();
    temporary_slots.sort_unstable();
    temporary_slots.dedup();
    if temporary_slots.len() != temporary_slot_count {
        return None;
    }

    if total_slots > 64
        || temporary_slots
            .iter()
            .any(|slot| *slot as u32 >= total_slots)
        || induction_cv as u32 >= op_array.num_cvs
        || accumulator_cv as u32 >= op_array.num_cvs
        || matches!(bound, QuickLongBound::Cv(cv) if cv as u32 >= op_array.num_cvs)
        || matches!(
            term,
            QuickLongTerm::InductionPlusCv { addend_cv, .. }
                if addend_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex { array_cv, .. }
                if array_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::StringLength { string_cv, .. }
                if string_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::AbsLong { operand_cv, .. }
                if operand_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex {
                index: QuickArrayIndex::ValueSlot(index_cv),
                ..
            } if index_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex {
                destination: Some(destination),
                ..
            } if destination as u32 >= op_array.num_cvs
        )
    {
        return None;
    }

    Some(QuickLongAccumulateLoop {
        header_ip,
        exit_ip,
        induction_cv,
        bound,
        accumulator_cv,
        condition_tmp,
        term,
        sum_tmp,
        tail_guard,
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
        native_jit: crate::jit::QuickLongAccumulateJitCache::new(),
    })
}

fn add_mask_slot(mask: &mut u64, slot: u16, total_slots: u32) -> Option<()> {
    if slot as u32 >= total_slots || slot >= 64 {
        return None;
    }
    *mask |= 1u64 << slot;
    Some(())
}

fn long_slot(op_type: OpType, slot: u16) -> Option<u16> {
    matches!(op_type, OpType::Cv | OpType::Tmp).then_some(slot)
}

fn quick_long_operand(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
) -> Option<QuickLongOperand> {
    match op_type {
        OpType::Cv | OpType::Tmp => Some(QuickLongOperand::Slot(operand)),
        OpType::Const => Some(QuickLongOperand::Const(long_literal(op_array, operand)?)),
        _ => None,
    }
}
