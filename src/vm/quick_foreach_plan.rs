//! Compile-time recognition for guarded value-only foreach regions.
//!
//! Runtime execution lives in `quick_foreach`; this module deliberately owns
//! only the target-neutral region contract and its structural proof.

use crate::compiler::OpArray;
use crate::value::Value;
use crate::vm::instruction::OpType;
use crate::vm::opcode::OpCode;

/// Scalar projection read from the object bound by a value-only foreach.
///
/// The canonical `FetchObjR` cache remains authoritative. The quick runner
/// only reuses its class/layout binding while every visited receiver matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickForeachObjectProjectionKind {
    Long,
    StringLength,
}

#[derive(Debug, Clone, Copy)]
pub struct QuickForeachObjectProjection {
    pub cache_ip: usize,
    pub kind: QuickForeachObjectProjectionKind,
    /// Temporary written by `FetchObjR`. For a Long projection this is also
    /// `result_tmp`; strlen owns a separate scalar result temporary.
    pub fetch_tmp: u16,
    pub result_tmp: u16,
}

/// Guarded value-only foreach recurrence over monomorphic object receivers:
///
/// ```php
/// foreach ($rows as $row) {
///     $accumulator += $row->value + strlen($row->name);
/// }
/// ```
///
/// One or two scalar projections are supported. Runtime binding handles both
/// declared property slots and canonical dynamic stdClass layouts; any class,
/// layout, type, reference, or overflow mismatch resumes the exact bytecode
/// instruction that still owns the PHP semantics.
#[derive(Debug, Clone, Copy)]
pub struct QuickForeachObjectPropertyAccumulateLoop {
    pub header_ip: usize,
    pub exit_ip: usize,
    pub array_tmp: u16,
    pub position_tmp: u16,
    pub receiver_cv: u16,
    pub done_tmp: u16,
    pub accumulator_cv: u16,
    pub projections: [Option<QuickForeachObjectProjection>; 2],
    pub projection_count: u8,
    /// Present when two projections are combined before accumulation.
    pub term_tmp: Option<u16>,
    pub term_ip: Option<usize>,
    pub sum_tmp: u16,
    pub sum_ip: usize,
}

fn foreach_object_projection(
    op_array: &OpArray,
    receiver_cv: u16,
    ip: usize,
) -> Option<(QuickForeachObjectProjection, usize)> {
    let fetch = *op_array.instructions.get(ip)?;
    if fetch.opcode != OpCode::FetchObjR
        || fetch.op1_type != OpType::Cv
        || fetch.op1 != receiver_cv
        || fetch.op2_type != OpType::Const
        || !matches!(fetch.result_type, OpType::Tmp | OpType::Var)
        || op_array
            .literals
            .get(fetch.op2 as usize)
            .and_then(Value::as_str)
            .is_none()
    {
        return None;
    }

    if let Some(strlen) = op_array.instructions.get(ip + 1).copied() {
        if matches!(strlen.opcode, OpCode::Strlen | OpCode::Strlen_String)
            && strlen.op1_type == fetch.result_type
            && strlen.op1 == fetch.result
            && matches!(strlen.result_type, OpType::Tmp | OpType::Var)
        {
            return Some((
                QuickForeachObjectProjection {
                    cache_ip: ip,
                    kind: QuickForeachObjectProjectionKind::StringLength,
                    fetch_tmp: fetch.result,
                    result_tmp: strlen.result,
                },
                ip + 2,
            ));
        }
    }

    Some((
        QuickForeachObjectProjection {
            cache_ip: ip,
            kind: QuickForeachObjectProjectionKind::Long,
            fetch_tmp: fetch.result,
            result_tmp: fetch.result,
        },
        ip + 1,
    ))
}

/// Recognize one closed value-only foreach body that projects one or two
/// scalar fields from the changing object receiver and accumulates them.
pub fn detect_foreach_object_property_accumulate_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickForeachObjectPropertyAccumulateLoop> {
    if backedge_ip >= op_array.instructions.len() || header_ip + 4 > backedge_ip {
        return None;
    }

    let next = op_array.instructions[header_ip];
    let branch = op_array.instructions[header_ip + 1];
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
        || !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
    {
        return None;
    }

    let receiver_cv = (next.extended_value & 0xffff) as u16;
    let (first, mut cursor) = foreach_object_projection(op_array, receiver_cv, header_ip + 2)?;
    let mut projections = [Some(first), None];
    let mut projection_count = 1u8;
    let mut term_tmp = None;
    let mut term_ip = None;
    let mut term_type = if first.kind == QuickForeachObjectProjectionKind::Long {
        op_array.instructions[first.cache_ip].result_type
    } else {
        op_array.instructions[first.cache_ip + 1].result_type
    };
    let mut term_slot = first.result_tmp;

    if op_array
        .instructions
        .get(cursor)
        .is_some_and(|instruction| instruction.opcode == OpCode::FetchObjR)
    {
        let (second, next_cursor) = foreach_object_projection(op_array, receiver_cv, cursor)?;
        let combine = *op_array.instructions.get(next_cursor)?;
        let second_type = if second.kind == QuickForeachObjectProjectionKind::Long {
            op_array.instructions[second.cache_ip].result_type
        } else {
            op_array.instructions[second.cache_ip + 1].result_type
        };
        if !matches!(
            combine.opcode,
            OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp
        ) || !matches!(combine.result_type, OpType::Tmp | OpType::Var)
            || !((combine.op1_type == term_type
                && combine.op1 == first.result_tmp
                && combine.op2_type == second_type
                && combine.op2 == second.result_tmp)
                || (combine.op2_type == term_type
                    && combine.op2 == first.result_tmp
                    && combine.op1_type == second_type
                    && combine.op1 == second.result_tmp))
        {
            return None;
        }
        projections[1] = Some(second);
        projection_count = 2;
        term_tmp = Some(combine.result);
        term_ip = Some(next_cursor);
        term_type = combine.result_type;
        term_slot = combine.result;
        cursor = next_cursor + 1;
    }

    let sum = *op_array.instructions.get(cursor)?;
    let assign = *op_array.instructions.get(cursor + 1)?;
    if cursor + 2 != backedge_ip
        || !matches!(
            sum.opcode,
            OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp
        )
        || !matches!(sum.result_type, OpType::Tmp | OpType::Var)
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op2_type != sum.result_type
        || assign.op2 != sum.result
        || assign.result_type != OpType::Unused
    {
        return None;
    }
    let accumulator_cv = assign.op1;
    if receiver_cv == accumulator_cv
        || !((sum.op1_type == OpType::Cv
            && sum.op1 == accumulator_cv
            && sum.op2_type == term_type
            && sum.op2 == term_slot)
            || (sum.op2_type == OpType::Cv
                && sum.op2 == accumulator_cv
                && sum.op1_type == term_type
                && sum.op1 == term_slot))
    {
        return None;
    }

    let exit_ip = branch.op2 as usize;
    if exit_ip <= backedge_ip || exit_ip >= op_array.instructions.len() {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if total_slots > 64
        || receiver_cv as u32 >= op_array.num_cvs
        || accumulator_cv as u32 >= op_array.num_cvs
    {
        return None;
    }
    let mut temporary_slots = vec![next.op1, next.op2, next.result];
    for projection in projections.iter().flatten() {
        temporary_slots.push(projection.fetch_tmp);
        if projection.result_tmp != projection.fetch_tmp {
            temporary_slots.push(projection.result_tmp);
        }
    }
    if let Some(term_tmp) = term_tmp {
        temporary_slots.push(term_tmp);
    }
    temporary_slots.push(sum.result);
    if temporary_slots
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

    Some(QuickForeachObjectPropertyAccumulateLoop {
        header_ip,
        exit_ip,
        array_tmp: next.op1,
        position_tmp: next.op2,
        receiver_cv,
        done_tmp: next.result,
        accumulator_cv,
        projections,
        projection_count,
        term_tmp,
        term_ip,
        sum_tmp: sum.result,
        sum_ip: cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile::Compiler;
    use crate::compiler::make_user_function;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    #[test]
    fn detects_foreach_object_property_projection_accumulation() {
        let source = "<?php
class Row { public $value; public $name; }
$rows = [];
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
";
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let result = Compiler::new().compile(&statements).unwrap();
        let main = make_user_function(result.main);
        let plan = main
            .op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .find_map(|(backedge, instruction)| {
                detect_foreach_object_property_accumulate_loop(
                    &main.op_array,
                    instruction.op1 as usize,
                    backedge,
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "source should contain a foreach object-property accumulation loop; instructions: {:#?}",
                    main.op_array.instructions
                )
            });
        assert_eq!(plan.projection_count, 2);
        assert!(matches!(
            plan.projections[0],
            Some(QuickForeachObjectProjection {
                kind: QuickForeachObjectProjectionKind::Long,
                ..
            })
        ));
        assert!(matches!(
            plan.projections[1],
            Some(QuickForeachObjectProjection {
                kind: QuickForeachObjectProjectionKind::StringLength,
                ..
            })
        ));
        assert!(plan.term_tmp.is_some());
        assert!(plan.term_ip.is_some());

        #[cfg(feature = "quick-loops")]
        assert!(main.op_array.block_plans.iter().any(|block_plan| matches!(
            block_plan,
            crate::vm::planner::BlockPlan::QuickForeachObjectPropertyAccumulate(_)
        )));
    }
}
