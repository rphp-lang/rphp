#[test]
fn real_php_branch_loop_enters_general_native_ir_region() {
    let source = "<?php $sum = 0; $bound = 100000; $cutoff = 50000; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $sum += $i; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:1249975000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_modulo_branch_loop_enters_general_native_ir_region() {
    let source = "<?php $sum = 0; $bound = 100000; $expected = 0; for ($i = 0; $i < $bound; $i++) { if (($i % 3) == $expected) { $sum += $i; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:1666683333"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the modulo Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_modulo_min_over_minus_one_preserves_canonical_semantics() {
    let source = "<?php function moduloLoop(int $start, int $bound): int { $sum = 0; for ($i = $start; $i < $bound; $i++) { if (($i % -1) == 0) { $sum += $i; } } return $sum; } moduloLoop(0, 100); echo moduloLoop(PHP_INT_MIN, PHP_INT_MIN + 1);";
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
        i64::MIN.to_string()
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("moduloLoop"))
        .map(|(_, function)| function)
        .expect("compiled moduloLoop function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("moduloLoop should use general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 2);
    // The VM executes the first iteration canonically before entering the hot
    // backedge region, so MIN % -1 is already resolved when native code sees
    // MIN + 1. The direct ABI test above covers the native guard itself.
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_straight_binary_body_enters_general_native_ir_region() {
    let source = "<?php $bound = 100000; $last = 0; $product = 0; $remaining = 0; for ($i = 0; $i < $bound; $i++) { $last = 20 + ($i % 400); $product = $i * 73; $remaining = $bound - $i; } echo $i . ':' . $last . ':' . $product . ':' . $remaining;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:419:7299927:1"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the straight Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn general_native_trace_guard_resumes_taken_cold_edge_transactionally() {
    let source = "<?php $needle = 74; $sum = 0; $count = 0; for ($i = 0; $i < 100; $i++) { $sum = $sum + $i; $count = $count + 1; if ($count === $needle) { echo 'hit:' . $i . ':' . $count . '|'; } } echo $sum . ':' . $count . ':' . $i;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "hit:73:74|4950:100:100"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("strict cold edge should retain the general Long loop IR");
    assert!(
        plan.ops
            .iter()
            .any(|operation| matches!(operation, QuickLongOp::TraceGuard { .. }))
    );
    assert!(plan.native_jit().is_straight_compiled());
    assert!(plan.native_jit().native_entries() >= 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_scalar_expression_chains_enter_general_native_ir_region() {
    let source = "<?php $bound = 100000; $left = 2; $right = 3; $literal = 0; $cv = 0; for ($i = 0; $i < $bound; $i++) { $literal = (($i * 73) + 20) - 7; $cv = $i + $left + $right; } echo $i . ':' . $literal . ':' . $cv;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:7299940:100004"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the scalar-expression Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_overlapping_scalar_lifetimes_enter_range_proven_native_region() {
    let source = "<?php $bound = 100000; $a = 0; $b = 0; $c = 0; $d = 0; for ($i = 0; $i < $bound; $i++) { $a = $i * 3; $b = $a + 7; $c = $a + $b; $d = $a + $b + $c; } echo $i . ':' . $a . ':' . $b . ':' . $c . ':' . $d;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:299997:300004:600001:1200002"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select overlapping scalar lifetimes");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_independent_recurrences_stay_in_range_proven_native_region() {
    let source = "<?php $bound = 100000; $sum = 10; $count = -5; $step = 2; for ($i = 0; $i < $bound; $i++) { $sum = $sum + $i; $count = $count + $step; } echo $i . ':' . $sum . ':' . $count;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:4999950010:199995"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the independent recurrence Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_carried_condition_recurrences_share_one_native_region() {
    let source = "<?php $bound = 100000; $cutoff = 49995; $sum = 10; $count = -5; for ($i = 0; $i < $bound; $i++) { if ($count < $cutoff) { $sum = $sum + $i; } $count = $count + 1; } echo $i . ':' . $sum . ':' . $count;";
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
        .expect("compiler should select the structured recurrence Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_conditional_composed_recurrence_delta_is_range_proven() {
    let source = "<?php $bound = 100000; $cutoff = 49995; $offset = 7; $sum = 10; $count = -5; for ($i = 0; $i < $bound; $i++) { if ($count < $cutoff) { $sum = $sum + (($i * 3) + $offset); } $count = $count + 1; } echo $i . ':' . $sum . ':' . $count;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:3750275010:99995"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the conditional composed recurrence IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_forward_dependent_recurrences_stay_in_one_native_region() {
    let source = "<?php $bound = 100000; $a = 3; $b = -7; for ($i = 0; $i < $bound; $i++) { $a = $a + 1; $b = $b + $a; } echo $i . ':' . $a . ':' . $b;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:100003:5000349993"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the dependent recurrence Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_reverse_order_dependency_preserves_old_value_semantics() {
    let source = "<?php $bound = 100000; $a = 3; $b = -7; for ($i = 0; $i < $bound; $i++) { $b = $b + $a; $a = $a + 1; } echo $i . ':' . $a . ':' . $b;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:100003:5000249993"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the reverse dependency Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_composed_recurrence_delta_stays_in_range_proven_native_region() {
    let source = "<?php $bound = 100000; $sum = 10; $offset = 7; for ($i = 0; $i < $bound; $i++) { $sum = $sum + (($i * 3) + $offset); } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:15000550010"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the composed recurrence Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_composed_recurrence_overflow_uses_precise_checked_side_exit() {
    let source = "<?php function composedDeltaOverflow(): int { $sum = 0; $factor = 92233720368547758; for ($i = 0; $i < 200; $i++) { $sum = $sum + (($i * $factor) - ($i * $factor)); } return $sum; } try { composedDeltaOverflow(); } catch (TypeError $error) { echo 'caught'; }";
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
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("composedDeltaOverflow"))
        .map(|(_, function)| function)
        .expect("compiled composedDeltaOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("overflowing composed recurrence should retain the Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_forward_scalar_branches_use_range_proven_native_region() {
    let source = "<?php $bound = 100000; $cutoff = 50000; $selected = 0; $folded = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $selected = ($i * 3) + 1; } else { $selected = ($i * 5) - 2; } $folded = ($selected * 3) + 11; } echo $i . ':' . $selected . ':' . $folded;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:499993:1499990"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the forward-branch Long loop IR");
    assert!(
        plan.native_jit().is_straight_compiled(),
        "forward branch did not select straight native IR: {:#?}",
        plan.ops
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_reversed_commutative_constants_use_range_proven_native_region() {
    let source = "<?php $bound = 100000; $cutoff = 50000; $selected = 0; $folded = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $selected = 1 + (3 * $i); } else { $selected = (5 * $i) - 2; } $folded = 11 + (3 * $selected); } echo $i . ':' . $selected . ':' . $folded;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:499993:1499990"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the reversed-commutative Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_runtime_invariant_arguments_share_native_registers() {
    let source = "<?php function runTwoInvariantLoop(int $bound, int $cutoff, int $offset): int { $selected = 0; $folded = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $selected = ($i * 3) + $offset; } else { $selected = ($i * 5) - $offset; } $folded = ($selected * 3) + $offset; } return $selected + $folded; } echo runTwoInvariantLoop(100000, 50000, 7);";
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
        "1999959"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runTwoInvariantLoop"))
        .map(|(_, function)| function)
        .expect("compiled two-invariant function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the two-invariant forward-branch IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_partially_written_branch_keeps_checked_native_chunks() {
    let source = "<?php $bound = 100000; $cutoff = 50000; $selected = 7; $folded = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $selected = $i * 3; } $folded = $selected + 1; } echo $i . ':' . $selected . ':' . $folded;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:149997:149998"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should retain the partially-written branch loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_amortized_safepoint_chunks(plan.native_jit().native_calls());
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_straight_binary_overflow_resumes_exact_canonical_operation() {
    let source = "<?php function binaryOverflow(): int { $value = PHP_INT_MAX - 40; $prefix = 0; for ($i = 0; $i < 100; $i++) { $prefix = $i + 1; $value = $value + 1; } return $prefix + $value; } try { binaryOverflow(); } catch (TypeError $error) { echo 'caught'; }";
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
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("binaryOverflow"))
        .map(|(_, function)| function)
        .expect("compiled binaryOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("binaryOverflow should use general Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn general_native_ir_handles_never_taken_add_and_exact_chunk_completion() {
    let source = "<?php $sum = 7; $bound = 65; $cutoff = 0; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $sum += $i; } } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "65:7"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn general_native_ir_sum_overflow_resumes_canonical_add() {
    let source = "<?php function conditionalOverflow(int $bound, int $cutoff): int { $sum = PHP_INT_MAX - 1000; for ($i = 0; $i < $bound; $i++) { if ($i < $cutoff) { $sum += $i; } } return $sum; } try { conditionalOverflow(60, 60); } catch (TypeError $error) { echo 'caught'; }";
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
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("conditionalOverflow"))
        .map(|(_, function)| function)
        .expect("compiled conditionalOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("conditionalOverflow should use general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn structured_recurrence_overflow_uses_checked_fallback() {
    let source = "<?php function structuredOverflow(int $bound, int $cutoff): int { $sum = PHP_INT_MAX - 3000; $count = 0; for ($i = 0; $i < $bound; $i++) { if ($count < $cutoff) { $sum = $sum + (($i * 3) + 7); } $count = $count + 1; } return $sum; } try { structuredOverflow(60, 60); } catch (TypeError $error) { echo 'caught'; }";
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
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("structuredOverflow"))
        .map(|(_, function)| function)
        .expect("compiled structuredOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("structuredOverflow should use general Long loop IR");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn general_native_modulo_ir_sum_overflow_resumes_canonical_add() {
    let source = "<?php function moduloOverflow(int $bound): int { $sum = PHP_INT_MAX - 2000; for ($i = 0; $i < $bound; $i++) { if (($i % 2) == 0) { $sum += $i; } } return $sum; } try { moduloOverflow(100); } catch (TypeError $error) { echo 'caught'; }";
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
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("moduloOverflow"))
        .map(|(_, function)| function)
        .expect("compiled moduloOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("moduloOverflow should use general Long loop IR");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().side_exits(), 1);
}
