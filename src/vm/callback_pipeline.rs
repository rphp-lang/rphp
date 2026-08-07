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

#[cfg(test)]
mod tests {
    use super::detect_callback_array_pipeline_span;
    use crate::compiler::compile::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::opcode::OpCode;

    fn compile(source: &str) -> crate::compiler::OpArray {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        Compiler::new().compile(&statements).unwrap().main
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
    fn rejects_dynamic_callback_and_materialized_stages() {
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

        let staged = compile(
            r#"<?php
$mapped = array_map("mapValue", $values);
$filtered = array_filter($mapped, "keepValue");
$result = array_reduce($filtered, "sumValue", 0);
"#,
        );
        assert!(
            staged
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, instruction)| instruction.opcode == OpCode::InitFcall)
                .all(|(ip, _)| detect_callback_array_pipeline_span(&staged, ip).is_none())
        );
    }
}
