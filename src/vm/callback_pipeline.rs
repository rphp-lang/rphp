use crate::compiler::OpArray;
use crate::value::Value;

use super::instruction::{Instruction, OpType};
use super::opcode::OpCode;

/// Compiler-proven bytecode span for the exact nested shape
/// `array_reduce(array_filter(array_map(...), ...), ..., initial)`.
///
/// The operand-producing expressions admitted here are deliberately simple.
/// Runtime may therefore evaluate pure scalar callback plans as one streaming
/// pass, or leave the untouched bytecode authoritative on any guard failure.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CallbackArrayPipelineSpan {
    pub(crate) map_callback: Instruction,
    pub(crate) source: Instruction,
    pub(crate) filter_callback: Instruction,
    pub(crate) reduce_callback: Instruction,
    pub(crate) initial: Instruction,
    pub(crate) do_fcall_ip: usize,
}

/// Target-neutral stage order for an admitted scalar collection pipeline.
/// Runtime dispatches this once before entering one of two monomorphic loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallbackArrayPipelineOrder {
    MapFilter,
    FilterMap,
}

/// Normalized program shared by nested and dead-staged bytecode detectors.
/// `discarded_cvs` is present only when execution suppresses canonical
/// intermediate assignments and therefore needs raw destination guards.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CallbackArrayPipelineProgram {
    pub(crate) span: CallbackArrayPipelineSpan,
    pub(crate) order: CallbackArrayPipelineOrder,
    pub(crate) discarded_cvs: Option<(u16, u16)>,
}

#[inline]
fn is_named_call(op_array: &OpArray, instruction: Instruction, name: &str, arity: u16) -> bool {
    instruction.opcode == OpCode::InitFcall
        && instruction.op1 == arity
        && instruction.op2_type == OpType::Const
        && instruction.extended_value == 0
        && op_array
            .literals
            .get(instruction.op2 as usize)
            .and_then(Value::as_str)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
}

#[inline]
fn is_positional_send(instruction: Instruction, position: u16) -> bool {
    instruction.opcode == OpCode::SendVal && instruction.op2 == position
}

#[inline]
fn is_string_literal_send(op_array: &OpArray, instruction: Instruction, position: u16) -> bool {
    is_positional_send(instruction, position)
        && instruction.op1_type == OpType::Const
        && op_array
            .literals
            .get(instruction.op1 as usize)
            .and_then(Value::as_str)
            .is_some()
}

/// Recognize only the fixed, side-effect-free operand envelope needed by the
/// first callback fusion. More general expressions and callable forms retain
/// the canonical nested call protocol.
pub(crate) fn detect_callback_array_pipeline_span(
    op_array: &OpArray,
    reduce_ip: usize,
) -> Option<CallbackArrayPipelineSpan> {
    let reduce = *op_array.instructions.get(reduce_ip)?;
    let filter = *op_array.instructions.get(reduce_ip + 1)?;
    let map = *op_array.instructions.get(reduce_ip + 2)?;
    if !is_named_call(op_array, reduce, "array_reduce", 3)
        || !is_named_call(op_array, filter, "array_filter", 2)
        || !is_named_call(op_array, map, "array_map", 2)
    {
        return None;
    }

    let map_callback = *op_array.instructions.get(reduce_ip + 3)?;
    let source = *op_array.instructions.get(reduce_ip + 4)?;
    let map_do = *op_array.instructions.get(reduce_ip + 5)?;
    let map_send = *op_array.instructions.get(reduce_ip + 6)?;
    let filter_callback = *op_array.instructions.get(reduce_ip + 7)?;
    let filter_do = *op_array.instructions.get(reduce_ip + 8)?;
    let filter_send = *op_array.instructions.get(reduce_ip + 9)?;
    let reduce_callback = *op_array.instructions.get(reduce_ip + 10)?;
    let initial = *op_array.instructions.get(reduce_ip + 11)?;
    let reduce_do = *op_array.instructions.get(reduce_ip + 12)?;

    if !is_string_literal_send(op_array, map_callback, 0)
        || !is_positional_send(source, 1)
        || !matches!(source.op1_type, OpType::Cv | OpType::Const)
        || map_do.opcode != OpCode::DoFcall
        || !matches!(map_do.result_type, OpType::Tmp | OpType::Var)
        || !is_positional_send(map_send, 0)
        || map_send.op1_type != map_do.result_type
        || map_send.op1 != map_do.result
        || !is_string_literal_send(op_array, filter_callback, 1)
        || filter_do.opcode != OpCode::DoFcall
        || !matches!(filter_do.result_type, OpType::Tmp | OpType::Var)
        || !is_positional_send(filter_send, 0)
        || filter_send.op1_type != filter_do.result_type
        || filter_send.op1 != filter_do.result
        || !is_string_literal_send(op_array, reduce_callback, 1)
        || !is_positional_send(initial, 2)
        || initial.op1_type != OpType::Const
        || op_array
            .literals
            .get(initial.op1 as usize)
            .is_none_or(|value| value.value_type() != crate::value::ValueType::Long)
        || reduce_do.opcode != OpCode::DoFcall
        || !matches!(
            reduce_do.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }

    Some(CallbackArrayPipelineSpan {
        map_callback,
        source,
        filter_callback,
        reduce_callback,
        initial,
        do_fcall_ip: reduce_ip + 12,
    })
}

/// Exact consecutive staged form whose two assigned arrays provably have no
/// observable consumers outside the next callback stage.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StagedCallbackArrayPipelineSpan {
    pub(crate) pipeline: CallbackArrayPipelineSpan,
    pub(crate) mapped_cv: u16,
    pub(crate) filtered_cv: u16,
}

#[inline]
fn assignment_destination(assignment: Instruction, source: Instruction) -> Option<u16> {
    (assignment.opcode == OpCode::AssignCv
        && assignment.op1_type == OpType::Cv
        && assignment.op2_type == source.result_type
        && assignment.op2 == source.result
        && assignment.result_type == OpType::Unused)
        .then_some(assignment.op1)
}

#[inline]
fn instruction_mentions_operand(instruction: &Instruction, op_type: OpType, slot: u16) -> bool {
    (instruction.op1_type == op_type && instruction.op1 == slot)
        || (instruction.op2_type == op_type && instruction.op2 == slot)
        || (instruction.result_type == op_type && instruction.result == slot)
}

fn has_only_cv_mentions(op_array: &OpArray, cv: u16, admitted_ips: [usize; 2]) -> bool {
    op_array
        .instructions
        .iter()
        .enumerate()
        .all(|(ip, instruction)| {
            admitted_ips.contains(&ip) || !instruction_mentions_operand(instruction, OpType::Cv, cv)
        })
}

/// Recognize a staged spelling only when both assigned intermediate arrays
/// have no syntactic use beyond feeding the immediately following stage.
pub(crate) fn detect_staged_callback_array_pipeline_span(
    op_array: &OpArray,
    map_ip: usize,
) -> Option<StagedCallbackArrayPipelineSpan> {
    // Main-scope CVs are mirrored into globals before later calls. Their
    // apparent local deadness is therefore insufficient for non-materializing
    // an assignment.
    if op_array.name == "<main>" {
        return None;
    }

    let map = *op_array.instructions.get(map_ip)?;
    let map_callback = *op_array.instructions.get(map_ip + 1)?;
    let source = *op_array.instructions.get(map_ip + 2)?;
    let map_do = *op_array.instructions.get(map_ip + 3)?;
    let map_assign = *op_array.instructions.get(map_ip + 4)?;
    let filter = *op_array.instructions.get(map_ip + 5)?;
    let filter_source = *op_array.instructions.get(map_ip + 6)?;
    let filter_callback = *op_array.instructions.get(map_ip + 7)?;
    let filter_do = *op_array.instructions.get(map_ip + 8)?;
    let filter_assign = *op_array.instructions.get(map_ip + 9)?;
    let reduce = *op_array.instructions.get(map_ip + 10)?;
    let reduce_source = *op_array.instructions.get(map_ip + 11)?;
    let reduce_callback = *op_array.instructions.get(map_ip + 12)?;
    let initial = *op_array.instructions.get(map_ip + 13)?;
    let reduce_do = *op_array.instructions.get(map_ip + 14)?;

    if !is_named_call(op_array, map, "array_map", 2)
        || !is_string_literal_send(op_array, map_callback, 0)
        || !is_positional_send(source, 1)
        || !matches!(source.op1_type, OpType::Cv | OpType::Const)
        || map_do.opcode != OpCode::DoFcall
        || !matches!(map_do.result_type, OpType::Tmp | OpType::Var)
        || !is_named_call(op_array, filter, "array_filter", 2)
        || !is_positional_send(filter_source, 0)
        || filter_source.op1_type != OpType::Cv
        || !is_string_literal_send(op_array, filter_callback, 1)
        || filter_do.opcode != OpCode::DoFcall
        || !matches!(filter_do.result_type, OpType::Tmp | OpType::Var)
        || !is_named_call(op_array, reduce, "array_reduce", 3)
        || !is_positional_send(reduce_source, 0)
        || reduce_source.op1_type != OpType::Cv
        || !is_string_literal_send(op_array, reduce_callback, 1)
        || !is_positional_send(initial, 2)
        || initial.op1_type != OpType::Const
        || op_array
            .literals
            .get(initial.op1 as usize)
            .is_none_or(|value| value.value_type() != crate::value::ValueType::Long)
        || reduce_do.opcode != OpCode::DoFcall
        || !matches!(
            reduce_do.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }

    let mapped_cv = assignment_destination(map_assign, map_do)?;
    let filtered_cv = assignment_destination(filter_assign, filter_do)?;
    if mapped_cv == filtered_cv
        || filter_source.op1 != mapped_cv
        || reduce_source.op1 != filtered_cv
        || !has_only_cv_mentions(op_array, mapped_cv, [map_ip + 4, map_ip + 6])
        || !has_only_cv_mentions(op_array, filtered_cv, [map_ip + 9, map_ip + 11])
    {
        return None;
    }

    Some(StagedCallbackArrayPipelineSpan {
        pipeline: CallbackArrayPipelineSpan {
            map_callback,
            source,
            filter_callback,
            reduce_callback,
            initial,
            do_fcall_ip: map_ip + 14,
        },
        mapped_cv,
        filtered_cv,
    })
}

/// Exact `array_filter` -> `array_map` -> `array_reduce` composition. Nested
/// syntax has no destinations; staged syntax may discard two proven-dead CVs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FilterMapCallbackArrayPipelineSpan {
    pub(crate) pipeline: CallbackArrayPipelineSpan,
    pub(crate) discarded_cvs: Option<(u16, u16)>,
}

fn detect_nested_filter_map_callback_array_pipeline_span(
    op_array: &OpArray,
    reduce_ip: usize,
) -> Option<FilterMapCallbackArrayPipelineSpan> {
    let reduce = *op_array.instructions.get(reduce_ip)?;
    let map = *op_array.instructions.get(reduce_ip + 1)?;
    let map_callback = *op_array.instructions.get(reduce_ip + 2)?;
    let filter = *op_array.instructions.get(reduce_ip + 3)?;
    let source = *op_array.instructions.get(reduce_ip + 4)?;
    let filter_callback = *op_array.instructions.get(reduce_ip + 5)?;
    let filter_do = *op_array.instructions.get(reduce_ip + 6)?;
    let filter_send = *op_array.instructions.get(reduce_ip + 7)?;
    let map_do = *op_array.instructions.get(reduce_ip + 8)?;
    let map_send = *op_array.instructions.get(reduce_ip + 9)?;
    let reduce_callback = *op_array.instructions.get(reduce_ip + 10)?;
    let initial = *op_array.instructions.get(reduce_ip + 11)?;
    let reduce_do = *op_array.instructions.get(reduce_ip + 12)?;

    if !is_named_call(op_array, reduce, "array_reduce", 3)
        || !is_named_call(op_array, map, "array_map", 2)
        || !is_string_literal_send(op_array, map_callback, 0)
        || !is_named_call(op_array, filter, "array_filter", 2)
        || !is_positional_send(source, 0)
        || !matches!(source.op1_type, OpType::Cv | OpType::Const)
        || !is_string_literal_send(op_array, filter_callback, 1)
        || filter_do.opcode != OpCode::DoFcall
        || !matches!(filter_do.result_type, OpType::Tmp | OpType::Var)
        || !is_positional_send(filter_send, 1)
        || filter_send.op1_type != filter_do.result_type
        || filter_send.op1 != filter_do.result
        || map_do.opcode != OpCode::DoFcall
        || !matches!(map_do.result_type, OpType::Tmp | OpType::Var)
        || !is_positional_send(map_send, 0)
        || map_send.op1_type != map_do.result_type
        || map_send.op1 != map_do.result
        || !is_string_literal_send(op_array, reduce_callback, 1)
        || !is_positional_send(initial, 2)
        || initial.op1_type != OpType::Const
        || op_array
            .literals
            .get(initial.op1 as usize)
            .is_none_or(|value| value.value_type() != crate::value::ValueType::Long)
        || reduce_do.opcode != OpCode::DoFcall
        || !matches!(
            reduce_do.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }

    Some(FilterMapCallbackArrayPipelineSpan {
        pipeline: CallbackArrayPipelineSpan {
            map_callback,
            source,
            filter_callback,
            reduce_callback,
            initial,
            do_fcall_ip: reduce_ip + 12,
        },
        discarded_cvs: None,
    })
}

fn detect_staged_filter_map_callback_array_pipeline_span(
    op_array: &OpArray,
    filter_ip: usize,
) -> Option<FilterMapCallbackArrayPipelineSpan> {
    if op_array.name == "<main>" {
        return None;
    }

    let filter = *op_array.instructions.get(filter_ip)?;
    let source = *op_array.instructions.get(filter_ip + 1)?;
    let filter_callback = *op_array.instructions.get(filter_ip + 2)?;
    let filter_do = *op_array.instructions.get(filter_ip + 3)?;
    let filter_assign = *op_array.instructions.get(filter_ip + 4)?;
    let map = *op_array.instructions.get(filter_ip + 5)?;
    let map_callback = *op_array.instructions.get(filter_ip + 6)?;
    let map_source = *op_array.instructions.get(filter_ip + 7)?;
    let map_do = *op_array.instructions.get(filter_ip + 8)?;
    let map_assign = *op_array.instructions.get(filter_ip + 9)?;
    let reduce = *op_array.instructions.get(filter_ip + 10)?;
    let reduce_source = *op_array.instructions.get(filter_ip + 11)?;
    let reduce_callback = *op_array.instructions.get(filter_ip + 12)?;
    let initial = *op_array.instructions.get(filter_ip + 13)?;
    let reduce_do = *op_array.instructions.get(filter_ip + 14)?;

    if !is_named_call(op_array, filter, "array_filter", 2)
        || !is_positional_send(source, 0)
        || !matches!(source.op1_type, OpType::Cv | OpType::Const)
        || !is_string_literal_send(op_array, filter_callback, 1)
        || filter_do.opcode != OpCode::DoFcall
        || !matches!(filter_do.result_type, OpType::Tmp | OpType::Var)
        || !is_named_call(op_array, map, "array_map", 2)
        || !is_string_literal_send(op_array, map_callback, 0)
        || !is_positional_send(map_source, 1)
        || map_source.op1_type != OpType::Cv
        || map_do.opcode != OpCode::DoFcall
        || !matches!(map_do.result_type, OpType::Tmp | OpType::Var)
        || !is_named_call(op_array, reduce, "array_reduce", 3)
        || !is_positional_send(reduce_source, 0)
        || reduce_source.op1_type != OpType::Cv
        || !is_string_literal_send(op_array, reduce_callback, 1)
        || !is_positional_send(initial, 2)
        || initial.op1_type != OpType::Const
        || op_array
            .literals
            .get(initial.op1 as usize)
            .is_none_or(|value| value.value_type() != crate::value::ValueType::Long)
        || reduce_do.opcode != OpCode::DoFcall
        || !matches!(
            reduce_do.result_type,
            OpType::Tmp | OpType::Var | OpType::Unused
        )
    {
        return None;
    }

    let filtered_cv = assignment_destination(filter_assign, filter_do)?;
    let mapped_cv = assignment_destination(map_assign, map_do)?;
    if filtered_cv == mapped_cv
        || map_source.op1 != filtered_cv
        || reduce_source.op1 != mapped_cv
        || !has_only_cv_mentions(op_array, filtered_cv, [filter_ip + 4, filter_ip + 7])
        || !has_only_cv_mentions(op_array, mapped_cv, [filter_ip + 9, filter_ip + 11])
    {
        return None;
    }

    Some(FilterMapCallbackArrayPipelineSpan {
        pipeline: CallbackArrayPipelineSpan {
            map_callback,
            source,
            filter_callback,
            reduce_callback,
            initial,
            do_fcall_ip: filter_ip + 14,
        },
        discarded_cvs: Some((filtered_cv, mapped_cv)),
    })
}

pub(crate) fn detect_filter_map_callback_array_pipeline_span(
    op_array: &OpArray,
    entry_ip: usize,
) -> Option<FilterMapCallbackArrayPipelineSpan> {
    detect_nested_filter_map_callback_array_pipeline_span(op_array, entry_ip)
        .or_else(|| detect_staged_filter_map_callback_array_pipeline_span(op_array, entry_ip))
}

#[cfg(test)]
mod tests {
    use super::{
        detect_callback_array_pipeline_span, detect_filter_map_callback_array_pipeline_span,
        detect_staged_callback_array_pipeline_span,
    };
    use crate::compiler::compile::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::opcode::OpCode;

    fn compile(source: &str) -> crate::compiler::OpArray {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        Compiler::new().compile(&statements).unwrap().main
    }

    fn compile_first_function(source: &str) -> crate::compiler::OpArray {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        Compiler::new()
            .compile(&statements)
            .unwrap()
            .functions
            .into_iter()
            .next()
            .unwrap()
            .1
            .op_array
    }

    #[test]
    fn detects_exact_nested_callback_pipeline() {
        let op_array = compile(
            r#"<?php
$result = array_reduce(
    array_filter(array_map("mapValue", $values), "keepValue"),
    "sumValue",
    0
);
"#,
        );
        let reduce_ip = op_array
            .instructions
            .iter()
            .position(|instruction| instruction.opcode == OpCode::InitFcall)
            .unwrap();
        let span = detect_callback_array_pipeline_span(&op_array, reduce_ip).unwrap();
        assert_eq!(span.do_fcall_ip, reduce_ip + 12);
    }

    #[test]
    fn detects_dead_staged_callback_pipeline() {
        let op_array = compile_first_function(
            r#"<?php
function pipeline($values) {
    $mapped = array_map("mapValue", $values);
    $filtered = array_filter($mapped, "keepValue");
    $result = array_reduce($filtered, "sumValue", 0);
    return $result;
}
"#,
        );
        let map_ip = op_array
            .instructions
            .iter()
            .position(|instruction| instruction.opcode == OpCode::InitFcall)
            .unwrap();
        let span = detect_staged_callback_array_pipeline_span(&op_array, map_ip).unwrap();
        assert_ne!(span.mapped_cv, span.filtered_cv);
        assert_eq!(span.pipeline.do_fcall_ip, map_ip + 14);
    }

    #[test]
    fn rejects_dynamic_callback_and_escaping_stages() {
        let dynamic = compile(
            r#"<?php
$result = array_reduce(
    array_filter(array_map($mapper, $values), "keepValue"),
    "sumValue",
    0
);
"#,
        );
        let dynamic_reduce = dynamic
            .instructions
            .iter()
            .position(|instruction| instruction.opcode == OpCode::InitFcall)
            .unwrap();
        assert!(detect_callback_array_pipeline_span(&dynamic, dynamic_reduce).is_none());

        let staged = compile_first_function(
            r#"<?php
function pipeline($values) {
    $mapped = array_map("mapValue", $values);
    $filtered = array_filter($mapped, "keepValue");
    $result = array_reduce($filtered, "sumValue", 0);
    echo count($mapped) . count($filtered);
    return $result;
}
"#,
        );
        assert!(
            staged
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, instruction)| instruction.opcode == OpCode::InitFcall)
                .all(|(ip, _)| detect_staged_callback_array_pipeline_span(&staged, ip).is_none())
        );
    }

    #[test]
    fn detects_nested_and_dead_staged_filter_map_pipeline() {
        let nested = compile(
            r#"<?php
$result = array_reduce(
    array_map("mapValue", array_filter($values, "keepValue")),
    "sumValue",
    0
);
"#,
        );
        let nested_ip = nested
            .instructions
            .iter()
            .position(|instruction| instruction.opcode == OpCode::InitFcall)
            .unwrap();
        let nested_span =
            detect_filter_map_callback_array_pipeline_span(&nested, nested_ip).unwrap();
        assert!(nested_span.discarded_cvs.is_none());
        assert_eq!(nested_span.pipeline.do_fcall_ip, nested_ip + 12);

        let staged = compile_first_function(
            r#"<?php
function pipeline($values) {
    $filtered = array_filter($values, "keepValue");
    $mapped = array_map("mapValue", $filtered);
    return array_reduce($mapped, "sumValue", 0);
}
"#,
        );
        let staged_ip = staged
            .instructions
            .iter()
            .position(|instruction| instruction.opcode == OpCode::InitFcall)
            .unwrap();
        let staged_span =
            detect_filter_map_callback_array_pipeline_span(&staged, staged_ip).unwrap();
        assert!(staged_span.discarded_cvs.is_some());
        assert_eq!(staged_span.pipeline.do_fcall_ip, staged_ip + 14);
    }

    #[test]
    fn rejects_escaping_filter_map_stages() {
        let op_array = compile_first_function(
            r#"<?php
function pipeline($values) {
    $filtered = array_filter($values, "keepValue");
    $mapped = array_map("mapValue", $filtered);
    $result = array_reduce($mapped, "sumValue", 0);
    echo count($filtered) . count($mapped);
    return $result;
}
"#,
        );
        assert!(
            op_array
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, instruction)| instruction.opcode == OpCode::InitFcall)
                .all(
                    |(ip, _)| detect_filter_map_callback_array_pipeline_span(&op_array, ip)
                        .is_none()
                )
        );
    }
}
