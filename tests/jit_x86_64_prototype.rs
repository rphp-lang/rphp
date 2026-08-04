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

#[test]
fn real_php_composed_constant_and_cv_terms_use_linear_native_ir() {
    let cases = [
        "<?php $n = 100000; $sum = 0; for ($i = 0; $i < $n; $i++) { $sum += $i + 7; } echo $i . ':' . $sum;",
        "<?php $n = 100000; $addend = 7; $sum = 0; for ($i = 0; $i < $n; $i++) { $sum += $i + $addend; } echo $i . ':' . $sum;",
    ];

    for source in cases {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();
        let main = make_user_function(compilation.main);
        let (mut globals, output) = common::make_eg_with_capture();
        execute::execute(&mut globals, &main).unwrap();
        drop(globals);
        assert_eq!(
            String::from_utf8(output.lock().unwrap().clone()).unwrap(),
            "100000:5000650000"
        );

        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickLongAccumulate(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should select a composed accumulate loop");
        assert!(plan.native_jit().is_straight_compiled());
        assert_eq!(plan.native_jit().native_entries(), 1);
        assert_eq!(plan.native_jit().native_calls(), 1);
        assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
        assert_eq!(plan.native_jit().side_exits(), 0);
    }
}

#[test]
fn real_php_forward_branch_uses_structured_native_ir() {
    let source = "<?php $n = 100000; $cutoff = 50000; $sum = 10; $count = -5; for ($i = 0; $i < $n; $i++) { if ($i < $cutoff) { $sum = $sum + $i; } $count = $count + 1; } echo $i . ':' . $sum . ':' . $count;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:1249975010:99995"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the structured scalar quick loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_modulo_branch_uses_native_arithmetic_and_control_flow() {
    let source = "<?php $n = 100000; $sum = 0; for ($i = 0; $i < $n; $i++) { if (($i % 2) == 0) { $sum += $i; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:2499950000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the modulo branch quick loop");
    assert!(
        plan.native_jit().is_straight_compiled(),
        "quick ops: {:#?}",
        plan.ops
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}
