/// Tests for parameter type hints
mod common;
use common::{run_php, run_php_expect_error};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
#[cfg(feature = "quick-loops")]
use rphp::vm::function::ScalarLongOpKind;
use rphp::vm::function::{
    CallStrategy, ComposedScalarLongOp, ComposedTypedLongOp, ReturnStrategy, ScalarLongCallGuard,
    ScalarStringSource,
};
use rphp::vm::instruction::{
    CALL_FLAG_EXACT_SCALAR_ARGS, CALL_FLAG_OBJECT_ARRAY_CONSUMERS, KnownScalarType,
    NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE,
};
use rphp::vm::opcode::OpCode;
#[cfg(feature = "quick-loops")]
use rphp::vm::planner::BlockPlan;
#[cfg(feature = "quick-loops")]
use rphp::vm::quick::{QuickLongOp, QuickTypedMethodCall};

fn compile_types(source: &str) -> rphp::compiler::compile::CompileResult {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new().compile(&statements).unwrap()
}

include!("e2e_type_hints/parameter_hints.rs");
include!("e2e_type_hints/return_hints.rs");
include!("e2e_type_hints/runtime_guards.rs");
include!("e2e_type_hints/scalar_propagation.rs");
include!("e2e_type_hints/string_plans.rs");
include!("e2e_type_hints/object_plans.rs");
include!("e2e_type_hints/static_property_hints.rs");
