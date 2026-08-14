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
    NEW_FLAG_VIRTUAL_DECLARED_READS, NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE,
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
include!("e2e_type_hints/instance_property_hints.rs");

#[test]
fn typed_by_reference_parameter_checks_the_referenced_value() {
    assert_eq!(
        run_php(
            "<?php function appendValue(array &$values): void { $values[] = 42; } $values = []; appendValue($values); echo $values[0];"
        ),
        "42"
    );
}
