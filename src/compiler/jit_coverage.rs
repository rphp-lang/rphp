//! Diagnostic-only classification for rejected quick/JIT regions.
//!
//! This module is compiled only with `vm-stats`; production planning stays in
//! `OpArray::prepare_quick_loops` without carrying reporting policy or tables.

use super::OpArray;
use crate::builtin_metadata::DirectInternalKind;
use crate::value::Value;
use crate::vm::instruction::Instruction;
use crate::vm::opcode::OpCode;
use crate::vm::stats::JitMissReason;

const CALLBACK_FUNCTIONS: &[&str] = &[
    "array_filter",
    "array_map",
    "array_reduce",
    "array_udiff",
    "array_udiff_assoc",
    "array_udiff_uassoc",
    "array_uintersect",
    "array_uintersect_assoc",
    "array_uintersect_uassoc",
    "array_walk",
    "array_walk_recursive",
    "iterator_apply",
    "preg_replace_callback",
    "preg_replace_callback_array",
    "uasort",
    "uksort",
    "usort",
];

fn named_call_matches(op_array: &OpArray, instruction: &Instruction, names: &[&str]) -> bool {
    if instruction.opcode != OpCode::InitFcall {
        return false;
    }
    let primary = op_array
        .literals
        .get(instruction.op2 as usize)
        .and_then(Value::as_str);
    let fallback = (instruction.extended_value != 0)
        .then(|| op_array.literals.get(instruction.extended_value as usize))
        .flatten()
        .and_then(Value::as_str);
    primary.into_iter().chain(fallback).any(|name| {
        let short_name = name.rsplit('\\').next().unwrap_or(name);
        names
            .iter()
            .any(|candidate| short_name.eq_ignore_ascii_case(candidate))
    })
}

/// Classify a rejected loop by its dominant architectural coverage gap.
pub(super) fn loop_miss_reason(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> JitMissReason {
    let mut has_callback_or_indirect_call = false;
    let mut has_array_shape = false;
    let mut has_string_shape = false;
    let mut has_object_shape = false;
    let mut has_direct_call_shape = false;
    let mut has_semantic_boundary = false;
    let mut branch_count = 0usize;

    for instruction in &op_array.instructions[header_ip..=backedge_ip] {
        if named_call_matches(op_array, instruction, &["json_encode", "json_decode"]) {
            return JitMissReason::JsonPipeline;
        }
        if instruction.opcode == OpCode::DirectInternalCall2
            && DirectInternalKind::from_id(instruction.extended_value)
                == Some(DirectInternalKind::JsonDecode)
        {
            return JitMissReason::JsonPipeline;
        }
        if named_call_matches(op_array, instruction, CALLBACK_FUNCTIONS) {
            has_callback_or_indirect_call = true;
        }

        match instruction.opcode {
            OpCode::CallUserFuncArray
            | OpCode::InitUserCall
            | OpCode::SendUser
            | OpCode::InitDynamicCall
            | OpCode::CreateClosure
            | OpCode::ClosureUseVar => has_callback_or_indirect_call = true,

            OpCode::InitArray
            | OpCode::AddArrayElement
            | OpCode::AddArrayUnpack
            | OpCode::FetchDimR
            | OpCode::AssignDim
            | OpCode::ArrayPushOp
            | OpCode::UnsetDim
            | OpCode::ForeachInit
            | OpCode::ForeachNext => has_array_shape = true,
            OpCode::ForeachNextPlain => has_array_shape = true,

            OpCode::Concat
            | OpCode::AssignConcat
            | OpCode::Strlen
            | OpCode::Concat_StringString
            | OpCode::Strlen_String
            | OpCode::Strlen_Cv => has_string_shape = true,

            OpCode::NewObj
            | OpCode::FetchObjR
            | OpCode::AssignObjProp
            | OpCode::InitMethodCall
            | OpCode::FetchStaticProp
            | OpCode::FetchLateStaticProp
            | OpCode::AssignStaticProp
            | OpCode::AssignLateStaticProp
            | OpCode::FetchClassConst
            | OpCode::FetchLateClassConst
            | OpCode::FetchDynamicClassConst
            | OpCode::FetchLateDynamicClassConst
            | OpCode::InitStaticCall
            | OpCode::InitLateStaticCall
            | OpCode::Instanceof
            | OpCode::AssignObjDim
            | OpCode::NullSafeCheck
            | OpCode::CloneObj
            | OpCode::UnsetObj => has_object_shape = true,

            OpCode::InitFcall
            | OpCode::DoFcall
            | OpCode::SendVal
            | OpCode::SendRef
            | OpCode::SendVarEx
            | OpCode::SendNamed
            | OpCode::DirectInternalCall1
            | OpCode::DirectInternalCall2 => has_direct_call_shape = true,

            OpCode::Throw
            | OpCode::Yield
            | OpCode::YieldFrom
            | OpCode::GeneratorReturn
            | OpCode::Include
            | OpCode::BindGlobal
            | OpCode::BindStatic
            | OpCode::FetchGlobals
            | OpCode::FetchGlobal
            | OpCode::AssignGlobal
            | OpCode::UnsetGlobal
            | OpCode::BindGlobalRef
            | OpCode::AssignGlobalRef => has_semantic_boundary = true,

            OpCode::JmpZ
            | OpCode::JmpNZ
            | OpCode::JmpZ_Le_CvConst
            | OpCode::JmpNZ_Le_CvConst
            | OpCode::JmpZ_Lt_CvConst
            | OpCode::JmpNZ_Lt_CvConst
            | OpCode::JmpZ_Eq_CvConst
            | OpCode::JmpNZ_Eq_CvConst => branch_count += 1,
            _ => {}
        }
    }

    if has_callback_or_indirect_call {
        JitMissReason::CallbackOrIndirectCall
    } else if has_array_shape {
        JitMissReason::ArrayShape
    } else if has_string_shape {
        JitMissReason::StringShape
    } else if has_object_shape {
        JitMissReason::ObjectShape
    } else if has_direct_call_shape {
        JitMissReason::DirectCallShape
    } else if has_semantic_boundary {
        JitMissReason::SemanticBoundary
    } else if branch_count > 1 {
        JitMissReason::ComplexControlFlow
    } else {
        JitMissReason::UnsupportedScalarShape
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn classify_loop(source: &str) -> JitMissReason {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let result = Compiler::new().compile(&statements).unwrap();
        let (backedge_ip, backedge) = result
            .main
            .instructions
            .iter()
            .enumerate()
            .find(|(ip, instruction)| {
                instruction.opcode == OpCode::Jmp && (instruction.op1 as usize) < *ip
            })
            .expect("test source must contain a backward loop edge");
        loop_miss_reason(&result.main, backedge.op1 as usize, backedge_ip)
    }

    #[test]
    fn prioritizes_json_pipelines_over_regular_calls() {
        assert_eq!(
            classify_loop("<?php for ($i = 0; $i < 10; $i++) { $row = json_encode($i); }"),
            JitMissReason::JsonPipeline
        );
    }

    #[test]
    fn recognizes_direct_associative_json_decode() {
        assert_eq!(
            classify_loop(
                r#"<?php $json = '{"value":1}'; for ($i = 0; $i < 10; $i++) { $row = json_decode($json, true); echo $row['value']; }"#
            ),
            JitMissReason::JsonPipeline
        );
    }

    #[test]
    fn prioritizes_callback_pipelines_over_array_shapes() {
        assert_eq!(
            classify_loop("<?php for ($i = 0; $i < 10; $i++) { $row = array_map('abs', [$i]); }"),
            JitMissReason::CallbackOrIndirectCall
        );
    }

    #[test]
    fn keeps_plain_numeric_gaps_separate() {
        assert_eq!(
            classify_loop(
                "<?php $value = 100.0; for ($i = 0; $i < 10; $i++) { $value = $value / 1.5; }"
            ),
            JitMissReason::UnsupportedScalarShape
        );
    }
}
