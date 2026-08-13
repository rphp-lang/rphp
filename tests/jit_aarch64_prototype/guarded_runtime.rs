#[test]
fn real_php_calls_enter_cached_native_plan_and_fallback_on_overflow() {
    let call_count = usize::from(SCALAR_LONG_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php function calc(int $a, int $b): int { return ($a + $b) * 3; } $total = 0;",
    );
    for _ in 0..call_count {
        source.push_str("$total = $total + calc(1, 2);");
    }
    source.push_str(
        "echo $total; try { calc(PHP_INT_MAX, 1); } catch (TypeError $error) { echo ':caught'; }",
    );

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
        "648:caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("calc"))
        .map(|(_, function)| function)
        .expect("compiled calc function");
    let plan = function.scalar_long_plan.as_deref().expect("scalar plan");
    assert!(plan.native_jit().is_compiled());
    assert!(plan.native_jit().native_entries() >= 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_conditional_calls_enter_the_standalone_native_plan() {
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
    assert!(plan.native_jit().native_entries() >= 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn cold_strict_branch_is_guarded_inside_the_native_call_region() {
    let source = "<?php function routeStandalone(int $value): int { if (($value & 1) == 0) { return ($value * 3) + 1; } return ($value * 5) - 2; } $total = 0; for ($i = 0; $i < 100; $i++) { $total += routeStandalone($i); if ($i === -1) { echo 'never'; } } echo $total;";
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
        "19800"
    );
    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("cold strict branch should retain the scalar-call accumulate region");
    assert!(plan.tail_guard.is_some());
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn taken_trace_guard_resumes_the_canonical_cold_block_before_increment() {
    let source = "<?php function routeGuarded(int $value): int { return ($value * 2) + 1; } $needle = 73; $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += routeGuarded($i); if ($i === $needle) { echo 'hit:' . $i . '|'; } } echo $i . ':' . $sum;";
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
        "hit:73|100:10000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("dynamic strict branch should use a guarded call region");
    assert!(plan.native_jit().is_call_compiled());
    assert!(plan.native_jit().native_entries() >= 1);
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:4999950000"
    );

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
    assert!(!plan.native_jit().is_call_compiled());
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "hit:73|100:4950"
    );

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

#[test]
fn native_accumulate_loop_preserves_chunk_and_overflow_boundaries() {
    let program = CompiledQuickLongAccumulateLoop::compile().expect("loop should lower to ARM64");
    let mut state = NativeLongAccumulateState {
        induction: 0,
        bound: 100,
        accumulator: 0,
    };

    assert_eq!(
        program.call(&mut state, 32).unwrap(),
        QuickLongAccumulateJitOutcome::ChunkExhausted
    );
    assert_eq!(state.induction, 32);
    assert_eq!(state.accumulator, 496);

    assert_eq!(
        program.call(&mut state, 64).unwrap(),
        QuickLongAccumulateJitOutcome::ChunkExhausted
    );
    assert_eq!(state.induction, 96);
    assert_eq!(state.accumulator, 4_560);

    assert_eq!(
        program.call(&mut state, 32).unwrap(),
        QuickLongAccumulateJitOutcome::Completed
    );
    assert_eq!(state.induction, 100);
    assert_eq!(state.accumulator, 4_950);

    let mut overflow = NativeLongAccumulateState {
        induction: 1,
        bound: 2,
        accumulator: i64::MAX,
    };
    assert_eq!(
        program.call(&mut overflow, 32).unwrap(),
        QuickLongAccumulateJitOutcome::SumOverflow
    );
    assert_eq!(
        overflow,
        NativeLongAccumulateState {
            induction: 1,
            bound: 2,
            accumulator: i64::MAX,
        },
        "overflow must not publish the wrapped ADD result"
    );
    assert!(matches!(
        program.call(&mut state, 0),
        Err(QuickLongAccumulateJitError::ZeroIterationBudget)
    ));
    assert!(!program.code().is_empty());

    let plus_one = CompiledQuickLongAccumulateLoop::compile_with_addend(1)
        .expect("constant term should lower to ARM64");
    let mut plus_one_state = NativeLongAccumulateState {
        induction: 0,
        bound: 10,
        accumulator: 0,
    };
    assert_eq!(
        plus_one.call(&mut plus_one_state, 32).unwrap(),
        QuickLongAccumulateJitOutcome::Completed
    );
    assert_eq!(plus_one_state.induction, 10);
    assert_eq!(plus_one_state.accumulator, 55);

    let plus_two = CompiledQuickLongAccumulateLoop::compile_with_addend(2)
        .expect("overflowing constant term should still lower transactionally");
    let mut term_overflow = NativeLongAccumulateState {
        induction: i64::MAX - 1,
        bound: i64::MAX,
        accumulator: 17,
    };
    assert_eq!(
        plus_two.call(&mut term_overflow, 32).unwrap(),
        QuickLongAccumulateJitOutcome::TermOverflow
    );
    assert_eq!(
        term_overflow,
        NativeLongAccumulateState {
            induction: i64::MAX - 1,
            bound: i64::MAX,
            accumulator: 17,
        },
        "term overflow must preserve the exact term instruction resume state"
    );
}

#[test]
fn real_php_accumulate_loop_enters_native_region() {
    let source =
        "<?php $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $i; } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:4999950000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select an accumulate quick loop");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn negative_accumulate_loop_uses_range_proven_native_chunks() {
    let source =
        "<?php $sum = 0; for ($i = -1000; $i < 1000; $i++) { $sum += $i; } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "1000:-1000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a negative accumulate quick loop");
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn real_php_guarded_scalar_method_enters_native_accumulate_region() {
    let source = "<?php class ScalarKernel { public function transform(int $value, int $scale): int { return ($value * $scale) + 7; } } $kernel = new ScalarKernel(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $kernel->transform($i, 73); } echo $i . ':' . $sum;";
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
        "100000:364997050000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "2500200000:2500200000:100000"
    );

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
    assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
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
        assert_eq!(
            String::from_utf8(output.lock().unwrap().clone()).unwrap(),
            expected
        );

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
        assert_amortized_safepoint_chunks(plan.native_jit().native_chunks());
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
            String::from_utf8(output.lock().unwrap().clone()).unwrap(),
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "hit:73|252000:252000:1000"
    );

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
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
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
            String::from_utf8(output.lock().unwrap().clone()).unwrap(),
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
            String::from_utf8(output.lock().unwrap().clone()).unwrap(),
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
fn native_property_method_replays_overflow_transaction_exactly_once() {
    let source = "<?php class NativePropertyLedger { public $count = 0; public $total = 9223372036854775707; public function record($value) { $this->count = $this->count + 1; $this->total = $this->total + $value; } } $ledger = new NativePropertyLedger(); for ($i = 0; $i < 1000; $i++) { $ledger->record(1); } echo $ledger->count . ':' . $i;";
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
        "1000:1000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the property method loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_property_method_rebinds_cached_program_to_each_activation() {
    let source = "<?php class NativeReboundLedger { public $total = 0; public function record($value) { $this->total = $this->total + $value; } } function runNativeReboundLedger($iterations) { $ledger = new NativeReboundLedger(); for ($i = 0; $i < $iterations; $i++) { $ledger->record($i); } return $ledger->total; } echo runNativeReboundLedger(1000) . ':' . runNativeReboundLedger(2000);";
    let tokens = Lexer::new(source).tokenize().unwrap();
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
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "499500:1999000"
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
        .expect("compiler should select the rebound property loop");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 2);
    assert_eq!(plan.native_jit().side_exits(), 0);
}
