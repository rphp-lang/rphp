#![cfg(all(feature = "jit-prototype", target_arch = "x86_64", target_os = "linux"))]

mod common;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::planner::BlockPlan;

#[test]
fn real_php_dynamic_bound_accumulate_uses_one_polling_native_call() {
    let source = "<?php $n = 100000; $sum = 10; for ($i = 0; $i < $n; $i++) { $sum = $sum + $i; } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:4999950010"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the accumulate quick loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert!((97..=99).contains(&plan.native_jit().native_chunks()));
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}
