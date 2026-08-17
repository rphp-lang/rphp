mod common;
use common::{run_php, run_php_with_source_context};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::function::CleanupMode;
use rphp::vm::instruction::OpType;
use rphp::vm::opcode::OpCode;

fn compile_source(source: &str) -> rphp::compiler::compile::CompileResult {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new().compile(&statements).unwrap()
}

fn main_opcodes(source: &str) -> Vec<OpCode> {
    compile_source(source)
        .main
        .instructions
        .iter()
        .map(|instruction| instruction.opcode)
        .collect()
}

include!("e2e_callable/string_callbacks_and_lowering.rs");

include!("e2e_callable/callback_array_shapes.rs");

include!("e2e_callable/callable_values_and_closures.rs");

include!("e2e_callable/argument_unpack_semantics.rs");

include!("e2e_callable/method_visibility_and_inheritance.rs");
