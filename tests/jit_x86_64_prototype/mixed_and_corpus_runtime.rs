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
fn real_php_read_only_dynamic_string_hash_lookups_enter_native_regions() {
    let cases = [
        (
            "literal assignments",
            "<?php $values = ['left' => 3, 'right' => 5]; $key = 'left'; $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $values[$key]; if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; } } echo $sum . ':' . $key . ':' . $i;",
            "400000:left:100000",
        ),
        (
            "guarded CV sources",
            "<?php $values = ['left' => 3, 'right' => 5]; $left = 'left'; $right = 'right'; $key = $left; $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $values[$key]; if (($i % 2) == 0) { $key = $right; } else { $key = $left; } } echo $sum . ':' . $key . ':' . $i;",
            "400000:left:100000",
        ),
        (
            "shared read-only array",
            "<?php $values = ['left' => 3, 'right' => 5]; $copy = $values; $key = 'left'; $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $values[$key]; if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; } } echo $sum . ':' . $key . ':' . $i . ':' . $copy['right'];",
            "400000:left:100000:5",
        ),
    ];

    for (label, source, expected) in cases {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();
        let main = make_user_function(compilation.main);
        let (mut globals, output) = common::make_eg_with_capture();

        execute::execute(&mut globals, &main).unwrap();
        drop(globals);
        assert_eq!(captured_output(&output), expected);

        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickLongOps(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should retain the read-only dynamic hash loop");
        assert!(
            plan.native_jit().is_straight_compiled(),
            "{label} should compile as one native straight region; ops={:#?}",
            plan.ops
        );
        assert_eq!(plan.native_jit().native_entries(), 1);
        assert!(plan.native_jit().native_chunks() > 1);
        assert_eq!(plan.native_jit().side_exits(), 0);
    }
}

#[test]
fn read_only_hash_context_rejects_missing_non_long_or_referenced_entry_before_native_entry() {
    let cases = [
        (
            "missing but unreachable payload",
            "<?php $values = ['left' => 1]; $key = 'left'; $last = 0; $sum = 0; for ($i = 0; $i < 100; $i++) { $last = $values[$key]; $sum += $i; if ($i < 0) { $key = 'missing'; } else { $key = 'left'; } } echo $sum . ':' . $last . ':' . $key . ':' . $i;",
            "4950:1:left:100",
        ),
        (
            "non-Long payload",
            "<?php $values = ['left' => 1, 'right' => 'marker']; $key = 'left'; $last = 0; $sum = 0; for ($i = 0; $i < 100; $i++) { $last = $values[$key]; $sum += $i; if ($i == 98) { $key = 'right'; } else { $key = 'left'; } } echo $sum . ':' . $last . ':' . $key . ':' . $i;",
            "4950:marker:left:100",
        ),
        (
            "referenced Long payload",
            "<?php $values = ['left' => 1, 'right' => 2]; $alias =& $values['right']; $key = 'left'; $last = 0; $sum = 0; for ($i = 0; $i < 100; $i++) { $last = $values[$key]; $sum += $i; if ($i == 98) { $key = 'right'; } else { $key = 'left'; } } echo $sum . ':' . $last . ':' . $key . ':' . $i . ':' . $alias;",
            "4950:2:left:100:2",
        ),
    ];

    for (label, source, expected) in cases {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();
        let main = make_user_function(compilation.main);
        let (mut globals, output) = common::make_eg_with_capture();

        execute::execute(&mut globals, &main).unwrap();
        drop(globals);
        assert_eq!(
            captured_output(&output),
            expected,
            "{label} should complete through the canonical path"
        );

        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickLongOps(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should retain the guarded read-only hash loop");
        assert_eq!(plan.native_jit().native_entries(), 0, "{label}");
        assert_eq!(plan.native_jit().side_exits(), 0, "{label}");
    }
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
    let source = include_str!("../../benches/holdout_routing_pipeline.php")
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
            include_str!("../../benches/corpus_order_pipeline.php"),
        ),
        (
            "runTypedQuotePipeline",
            include_str!("../../benches/corpus_typed_order_pipeline.php"),
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
            include_str!("../../benches/corpus_ledger_pipeline.php"),
        ),
        (
            "runTypedLedgerPipeline",
            include_str!("../../benches/corpus_typed_ledger_pipeline.php"),
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
