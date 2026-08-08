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
