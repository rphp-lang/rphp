/// Inline a proven scalar leaf body into the quick call-site argument program.
/// Body inputs become the argument program's outputs and body temporaries are
/// shifted after argument-expression temporaries. The result is still the same
/// context-independent scalar IR, now executable as one guarded region.
pub(crate) fn compose_quick_scalar_leaf_program(
    arguments: &ScalarLongProgram,
    body: &ScalarLongFunctionPlan,
) -> Option<ScalarLongProgram<ScalarLongOp, 1>> {
    const MAX_FUSED_SCALAR_OPS: usize = 16;

    if arguments.output_count != body.public_args
        || body.select.is_some()
        || arguments.operations.len() > 8
        || body.program.operations.len() > 8
        || body.program.output_count != 1
        || arguments.operations.len() + body.program.operations.len() > MAX_FUSED_SCALAR_OPS
    {
        return None;
    }
    let argument_operation_count = arguments.operations.len();
    let remap_body_source = |source| match source {
        ScalarLongSource::Input(index) => arguments
            .outputs
            .get(index as usize)
            .copied()
            .filter(|_| index < u16::from(arguments.output_count)),
        ScalarLongSource::Constant(value) => Some(ScalarLongSource::Constant(value)),
        ScalarLongSource::Temporary(index) => {
            let index = argument_operation_count.checked_add(index as usize)?;
            u8::try_from(index).ok().map(ScalarLongSource::Temporary)
        }
    };

    let mut operations =
        Vec::with_capacity(argument_operation_count + body.program.operations.len());
    operations.extend(arguments.operations.iter().copied());
    for operation in body.program.operations.iter().copied() {
        operations.push(ScalarLongOp {
            kind: operation.kind,
            lhs: remap_body_source(operation.lhs)?,
            rhs: remap_body_source(operation.rhs)?,
        });
    }
    let result = remap_body_source(body.program.outputs[0])?;
    Some(ScalarLongProgram {
        operations: operations.into_boxed_slice(),
        outputs: [result],
        output_count: 1,
    })
}

fn array_literal_index(op_array: &OpArray, index: u16) -> Option<QuickArrayIndex> {
    let value = op_array.literals.get(index as usize)?;
    if let Some(value) = value.as_long() {
        return Some(QuickArrayIndex::Long(QuickLongOperand::Const(value)));
    }

    let value = value.as_str()?;
    if let Ok(integer) = value.parse::<i64>() {
        if integer.to_string() == value {
            return Some(QuickArrayIndex::Long(QuickLongOperand::Const(integer)));
        }
    }
    Some(QuickArrayIndex::StringLiteral(index))
}

/// Recognize a value-only foreach loop whose complete body adds each long
/// value to one long accumulator.
pub fn detect_foreach_long_accumulate_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickForeachLongAccumulateLoop> {
    if header_ip.checked_add(4)? != backedge_ip || backedge_ip >= op_array.instructions.len() {
        return None;
    }

    let next = op_array.instructions[header_ip];
    let branch = op_array.instructions[header_ip + 1];
    let sum = op_array.instructions[header_ip + 2];
    let assign = op_array.instructions[header_ip + 3];
    let backedge = op_array.instructions[backedge_ip];

    if next.opcode != OpCode::ForeachNext
        || next.op1_type != OpType::Tmp
        || next.op2_type != OpType::Tmp
        || next.result_type != OpType::Tmp
        || next.extended_value >> 16 != 0
        || branch.opcode != OpCode::JmpZ
        || branch.op1_type != OpType::Tmp
        || branch.op1 != next.result
        || branch.op2_type != OpType::Unused
        || sum.opcode != OpCode::Add
        || sum.op1_type != OpType::Cv
        || sum.op2_type != OpType::Cv
        || sum.result_type != OpType::Tmp
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op2_type != OpType::Tmp
        || assign.op2 != sum.result
        || assign.result_type != OpType::Unused
        || !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
    {
        return None;
    }

    let value_cv = (next.extended_value & 0xffff) as u16;
    let accumulator_cv = if sum.op1 == value_cv && sum.op2 != value_cv {
        sum.op2
    } else if sum.op2 == value_cv && sum.op1 != value_cv {
        sum.op1
    } else {
        return None;
    };
    if assign.op1 != accumulator_cv || value_cv == accumulator_cv {
        return None;
    }

    let exit_ip = branch.op2 as usize;
    if exit_ip <= backedge_ip || exit_ip >= op_array.instructions.len() {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    let temporary_slots = [next.op1, next.op2, next.result, sum.result];
    if total_slots > 64
        || value_cv as u32 >= op_array.num_cvs
        || accumulator_cv as u32 >= op_array.num_cvs
        || temporary_slots
            .iter()
            .any(|slot| (*slot as u32) < op_array.num_cvs || (*slot as u32) >= total_slots)
        || temporary_slots.iter().enumerate().any(|(index, slot)| {
            temporary_slots[index + 1..]
                .iter()
                .any(|other| other == slot)
        })
    {
        return None;
    }

    Some(QuickForeachLongAccumulateLoop {
        header_ip,
        exit_ip,
        array_tmp: next.op1,
        position_tmp: next.op2,
        value_cv,
        done_tmp: next.result,
        accumulator_cv,
        sum_tmp: sum.result,
        sum_ip: header_ip + 2,
    })
}

/// Recognize a side-effect-free loop whose only body operation increments the
/// induction variable.
pub fn detect_long_induction_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickLongInductionLoop> {
    if backedge_ip >= op_array.instructions.len()
        || !matches!(
            backedge_ip.checked_sub(header_ip),
            Some(3) | Some(4)
        )
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

    let increment_ip = header_ip + 2;
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

    if backedge_ip == header_ip + 4 {
        let release = op_array.instructions[header_ip + 3];
        if release.opcode != OpCode::ReleaseTemps
            || release.op1_type != OpType::Tmp
            || release.op2_type != OpType::Tmp
            || release.result_type != OpType::Unused
            || release.op1 > release.op2
        {
            return None;
        }
    }

    if !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
        || matches!(bound, QuickLongBound::Cv(cv) if cv == induction_cv)
    {
        return None;
    }

    if condition_tmp.is_some() && condition_tmp == increment_tmp {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if total_slots > 64
        || induction_cv as u32 >= op_array.num_cvs
        || matches!(bound, QuickLongBound::Cv(cv) if cv as u32 >= op_array.num_cvs)
        || condition_tmp.is_some_and(|slot| slot as u32 >= total_slots)
        || increment_tmp.is_some_and(|slot| slot as u32 >= total_slots)
    {
        return None;
    }

    Some(QuickLongInductionLoop {
        header_ip,
        exit_ip,
        induction_cv,
        bound,
        condition_tmp,
        increment_kind,
        increment_tmp,
        increment_ip,
    })
}
