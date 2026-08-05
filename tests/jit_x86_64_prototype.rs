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

#[test]
fn real_php_exact_float_calls_enter_double_jit_and_long_inputs_fallback() {
    let call_count = usize::from(SCALAR_DOUBLE_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php function blend(float $a, float $b, float $c): float { return (($a + 1.5) * $b) / $c; } $total = 0.0;",
    );
    for _ in 0..call_count {
        source.push_str("$total = $total + blend(2.5, 4.0, 2.0);");
    }
    source.push_str("echo $total . ':' . blend(2, 4, 2);");

    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(captured_output(&output), "576:7");

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("blend"))
        .map(|(_, function)| function)
        .expect("compiled blend function");
    let plan = function
        .scalar_double_plan
        .as_deref()
        .expect("Double scalar plan");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 9);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn monomorphic_float_method_uses_class_cache_and_double_jit() {
    let call_count = usize::from(SCALAR_DOUBLE_JIT_HOT_THRESHOLD) + 8;
    let source = format!(
        "<?php class FloatModel {{ public function blend(float $a, float $b, float $c): float {{ return (($a + 1.5) * $b) / $c; }} }} $model = new FloatModel(); $total = 0.0; for ($i = 0; $i < {call_count}; $i++) {{ $total += $model->blend(2.5, 4.0, 2.0); }} echo $total;"
    );
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    assert_eq!(captured_output(&output), "576");

    let class = globals
        .class_table
        .values()
        .find(|class| class.name.eq_ignore_ascii_case("FloatModel"))
        .expect("registered FloatModel");
    let method = class
        .methods
        .iter()
        .find(|(name, ..)| name.eq_ignore_ascii_case("blend"))
        .map(|(_, _, _, _, method)| method)
        .expect("compiled blend method");
    let plan = method
        .scalar_double_plan
        .as_deref()
        .expect("Double scalar method plan");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 8);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn double_jit_zero_divisor_replays_canonical_php_error() {
    let call_count = usize::from(SCALAR_DOUBLE_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php function divideFloat(float $value, float $divisor): float { return ($value + 1.0) / $divisor; }",
    );
    for _ in 0..call_count {
        source.push_str("divideFloat(7.0, 2.0);");
    }
    source.push_str("divideFloat(7.0, 0.0);");

    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, _output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    let error = execute::execute(&mut globals, &main).unwrap_err();
    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message) if message == "Division by zero"
    ));

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("divideFloat"))
        .map(|(_, function)| function)
        .expect("compiled divideFloat function");
    let plan = function
        .scalar_double_plan
        .as_deref()
        .expect("Double scalar plan");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 10);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

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
fn real_php_constant_bound_keeps_three_recurrences_and_an_invariant_native() {
    let source = "<?php $offset = 5; $left = 1; $middle = 2; $right = 3; for ($i = 0; $i < 100000; $i++) { $left = $left + $offset; $middle = $middle + $offset; $right = $right + $offset; } echo $i . ':' . $left . ':' . $middle . ':' . $right;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(captured_output(&output), "100000:500001:500002:500003");

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the three-recurrence typed loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_finite_string_method_and_hash_update_enter_one_native_region() {
    let source = "<?php class MixedNativeModel { public function score(int $value, string $key): int { return $value + strlen($key); } } $model = new MixedNativeModel(); $values = ['left' => 0, 'right' => 0]; $key = 'left'; $needle = -1; for ($i = 0; $i < 100000; $i++) { if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; } $score = $model->score($i, $key); $values[$key] = $values[$key] + $score; if ($i === $needle) { echo 'never'; } } echo $values['left'] . ':' . $values['right'] . ':' . $i;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(captured_output(&output), "2500200000:2500200000:100000");

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a mixed typed loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn native_mixed_hash_region_replays_taken_cold_edge_after_prior_store() {
    let source = "<?php class MixedColdModel { public function score(int $value, string $key): int { return $value + strlen($key); } } $model = new MixedColdModel(); $values = ['left' => 0, 'right' => 0]; $key = 'left'; $needle = 73; for ($i = 0; $i < 1000; $i++) { if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; } $score = $model->score($i, $key); $values[$key] = $values[$key] + $score; if ($i === $needle) { echo 'hit:' . $i . '|'; } } echo $values['left'] . ':' . $values['right'] . ':' . $i;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(captured_output(&output), "hit:73|252000:252000:1000");

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should retain the mixed cold-edge region");
    assert!(plan.native_jit().native_entries() >= 2);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_routing_holdout_enters_multi_method_native_region() {
    let source = include_str!("../benches/holdout_routing_pipeline.php")
        .replace("$start = microtime(true);", "")
        .replace("$elapsed = microtime(true) - $start;", "$elapsed = 0;");
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        captured_output(&output),
        "290394364,154183816,54660174,384960,192495,64134,108411|0"
    );
    let plan = functions
        .iter()
        .find_map(|(_, function)| {
            function
                .op_array
                .block_plans
                .iter()
                .find_map(|plan| match plan {
                    BlockPlan::QuickLongOps(plan) => Some(plan),
                    _ => None,
                })
        })
        .expect("compiler should select the routing holdout as one typed loop");
    assert_eq!(plan.ops.len(), 28);
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn application_order_corpus_enters_virtual_pipeline_native_region() {
    for (function_name, original) in [
        (
            "runQuotePipeline",
            include_str!("../benches/corpus_order_pipeline.php"),
        ),
        (
            "runTypedQuotePipeline",
            include_str!("../benches/corpus_typed_order_pipeline.php"),
        ),
    ] {
        let source = original
            .replace("$start = microtime(true);", "")
            .replace("$elapsed = microtime(true) - $start;", "$elapsed = 0;");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();
        let main = make_user_function(compilation.main);
        let functions = compilation.functions;
        let class_defs = compilation.class_defs;
        let (mut globals, output) = common::make_eg_with_capture();
        for (name, function) in &functions {
            globals
                .register_function(name, &function.common as *const FunctionCommon)
                .unwrap();
        }
        for class_def in class_defs {
            globals.register_class(class_def).unwrap();
        }

        execute::execute(&mut globals, &main).unwrap();
        drop(globals);
        assert_eq!(
            captured_output(&output),
            "9895778000,1327440292,11223218292,210000|0"
        );

        let function = functions
            .iter()
            .find_map(|(name, function)| (name == function_name).then_some(function))
            .expect("corpus function should be compiled");
        let plan = function
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickLongOps(plan)
                    if plan.ops.iter().any(|operation| {
                        matches!(operation, QuickLongOp::VirtualObjectArrayPipeline { .. })
                    }) =>
                {
                    Some(plan)
                }
                _ => None,
            })
            .expect("compiler should select the virtual object-array pipeline");
        assert!(plan.native_jit().is_straight_compiled());
        assert_eq!(plan.native_jit().native_entries(), 1);
        assert!(plan.native_jit().native_chunks() > 1);
        assert_eq!(plan.native_jit().side_exits(), 0);
    }
}

#[test]
fn application_ledger_corpus_enters_property_native_region() {
    for (function_name, original) in [
        (
            "runLedgerPipeline",
            include_str!("../benches/corpus_ledger_pipeline.php"),
        ),
        (
            "runTypedLedgerPipeline",
            include_str!("../benches/corpus_typed_ledger_pipeline.php"),
        ),
    ] {
        let source = original
            .replace("$start = microtime(true);", "")
            .replace("$elapsed = microtime(true) - $start;", "$elapsed = 0;");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();
        let main = make_user_function(compilation.main);
        let functions = compilation.functions;
        let class_defs = compilation.class_defs;
        let (mut globals, output) = common::make_eg_with_capture();
        for (name, function) in &functions {
            globals
                .register_function(name, &function.common as *const FunctionCommon)
                .unwrap();
        }
        for class_def in class_defs {
            globals.register_class(class_def).unwrap();
        }

        execute::execute(&mut globals, &main).unwrap();
        drop(globals);
        assert_eq!(
            captured_output(&output),
            "500000,7981250000,280500000,182500|0"
        );

        let function = functions
            .iter()
            .find_map(|(name, function)| (name == function_name).then_some(function))
            .expect("corpus function should be compiled");
        let plan = function
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickLongOps(plan)
                    if plan.ops.iter().any(|operation| {
                        matches!(operation, QuickLongOp::PropertyMethodCall { .. })
                    }) =>
                {
                    Some(plan)
                }
                _ => None,
            })
            .expect("compiler should select the stateful property pipeline");
        assert!(plan.native_jit().is_straight_compiled());
        assert_eq!(plan.native_jit().native_entries(), 1);
        assert!(plan.native_jit().native_chunks() > 1);
        assert_eq!(plan.native_jit().side_exits(), 0);
    }
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

#[test]
fn real_php_conditional_calls_enter_standalone_scalar_jit() {
    let call_count = usize::from(SCALAR_LONG_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php function route(int $value): int { if (($value & 1) == 0) { return ($value * 3) + 1; } return ($value * 5) - 2; } $total = 0;",
    );
    for index in 0..call_count {
        source.push_str(if index & 1 == 0 {
            "$total = $total + route(4);"
        } else {
            "$total = $total + route(5);"
        });
    }
    source.push_str("echo $total;");

    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "1296"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("route"))
        .map(|(_, function)| function)
        .expect("compiled route function");
    let plan = function.scalar_long_plan.as_deref().expect("scalar plan");
    assert!(plan.select.is_some());
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 9);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_scalar_call_is_composed_into_native_accumulate_loop() {
    let source = "<?php function calculate(int $value): int { return ($value * 2) + 1; } $n = 100000; $sum = 0; for ($i = 0; $i < $n; $i++) { $sum += calculate($i); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:10000000000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the scalar-call accumulate loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!((97..=99).contains(&plan.native_jit().native_calls()));
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_scalar_call_tree_enters_one_native_accumulate_region() {
    let source = "<?php function addNative(int $left, int $right): int { return $left + $right; } function mulNative(int $left, int $right): int { return $left * $right; } $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += addNative($i + 1, mulNative($i, 2)); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:14999950000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the nested scalar-call loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!((97..=99).contains(&plan.native_jit().native_calls()));
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_scalar_method_tree_enters_one_native_accumulate_region() {
    let source = "<?php class MathTree { public function add($left, $right) { return $left + $right; } public function mul($left, $right) { return $left * $right; } } $math = new MathTree(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $math->add($i, $math->mul($i, 2)); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let class_defs = compilation.class_defs;
    let (mut globals, output) = common::make_eg_with_capture();
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:14999850000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the nested scalar-method loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!((97..=99).contains(&plan.native_jit().native_calls()));
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn scalar_call_overflow_side_exit_replays_canonical_root_call() {
    let source = "<?php function overflowNative(int $value): int { return ($value * 100000000000000000) % 7; } function runFunctionOverflow(): int { $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += overflowNative($i); } return $sum; } runFunctionOverflow();";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }

    let error = execute::execute(&mut globals, &main).unwrap_err();
    drop(globals);
    assert!(matches!(
        error,
        execute::VmError::Fatal(message)
            if message == "Unsupported operand types for %"
    ));
    assert!(output.lock().unwrap().is_empty());

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runFunctionOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runFunctionOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runFunctionOverflow should use a scalar-function loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn cold_simple_accumulate_guard_stays_inside_the_native_region() {
    let source = "<?php $needle = -1; $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $i; if ($i === $needle) { echo 'never'; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(captured_output(&output), "100000:4999950000");

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("cold branch should retain the simple accumulate region");
    assert!(plan.tail_guard.is_some());
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 98);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn guarded_invariant_term_is_composed_into_one_native_call() {
    let source = "<?php $offset = 7; $needle = -1; $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $i + $offset; if ($i === $needle) { echo 'never'; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(captured_output(&output), "100000:5000650000");

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("guarded invariant term should retain the accumulate region");
    assert!(plan.tail_guard.is_some());
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn taken_simple_accumulate_guard_replays_the_cold_block() {
    let source = "<?php $needle = 73; $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += $i; if ($i === $needle) { echo 'hit:' . $i . '|'; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(captured_output(&output), "hit:73|100:4950");

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("taken branch should retain the guarded accumulate region");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}
