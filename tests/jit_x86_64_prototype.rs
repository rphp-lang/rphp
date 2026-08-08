#![cfg(all(feature = "jit-prototype", target_arch = "x86_64", target_os = "linux"))]

mod common;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::jit::{SCALAR_DOUBLE_JIT_HOT_THRESHOLD, SCALAR_LONG_JIT_HOT_THRESHOLD};
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::function::FunctionCommon;
use rphp::vm::planner::BlockPlan;
use rphp::vm::quick::QuickLongOp;

fn captured_output(output: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    String::from_utf8(output.lock().unwrap().clone()).unwrap()
}

include!("jit_x86_64_prototype/double_calls.rs");
include!("jit_x86_64_prototype/double_composition.rs");
include!("jit_x86_64_prototype/mixed_and_corpus_runtime.rs");
include!("jit_x86_64_prototype/scalar_calls_and_guards.rs");
