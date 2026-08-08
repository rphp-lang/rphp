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
fn real_php_typed_double_call_accumulation_enters_one_native_region() {
    let source = "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateFloat(1.5, 2.5, $scale); } echo $i . ':' . $total;";
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
    assert_eq!(captured_output(&output), "100000:600000");

    let loop_plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a Double call/accumulate loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let leaf = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("calculateFloat"))
        .and_then(|(_, function)| function.scalar_double_plan.as_deref())
        .expect("Double leaf plan");
    assert!(!leaf.native_jit().is_compiled());
}

#[test]
fn conditional_typed_double_call_accumulation_enters_one_native_region() {
    let source = "<?php function conditionalFloat(float $value, float $pivot): float { if ($value < $pivot) { return ($value * 1.5) + 2.0; } return ($value * 0.5) - 1.0; } $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += conditionalFloat($i * 0.5, 25000.0); } echo $i . ':' . $total;";
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
    assert_eq!(captured_output(&output), "100000:1875025000");

    let loop_plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("conditional Double call/accumulate loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let leaf = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("conditionalFloat"))
        .and_then(|(_, function)| function.scalar_double_plan.as_deref())
        .expect("conditional Double leaf plan");
    assert!(leaf.select.is_some());
    assert!(!leaf.native_jit().is_compiled());
}

#[test]
fn monomorphic_typed_double_method_enters_one_native_region() {
    let source = "<?php class FloatCalculator { public function calculate(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } } $calculator = new FloatCalculator(); $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += $calculator->calculate(1.5, 2.5, 2.0); } echo $i . ':' . $total;";
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
    assert_eq!(captured_output(&output), "100000:600000");

    let loop_plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a guarded Double method loop");
    assert!(matches!(
        loop_plan.guard,
        rphp::vm::function::ScalarLongCallGuard::MethodCache { .. }
    ));
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let class = globals
        .class_table
        .values()
        .find(|class| class.name.eq_ignore_ascii_case("FloatCalculator"))
        .expect("registered FloatCalculator");
    let method = class
        .methods
        .iter()
        .find(|(name, ..)| name.eq_ignore_ascii_case("calculate"))
        .map(|(_, _, _, _, method)| method)
        .expect("compiled calculate method");
    assert_eq!(method.common.call_count.get(), 100000);
    let leaf = method
        .scalar_double_plan
        .as_deref()
        .expect("Double method leaf plan");
    assert!(!leaf.native_jit().is_compiled());
}

#[test]
fn conditional_typed_double_method_enters_one_native_region() {
    let source = "<?php class ConditionalFloat { public function apply(float $value, float $pivot): float { $scaled = $value * 1.0; if ($scaled < $pivot) { $result = ($scaled * 1.5) + 2.0; return $result; } $result = ($scaled * 0.5) - 1.0; return $result; } } $calculator = new ConditionalFloat(); $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += $calculator->apply($i * 0.5, 25000.0); } echo $i . ':' . $total;";
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
    assert_eq!(captured_output(&output), "100000:1875025000");

    let loop_plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("conditional guarded Double method loop");
    assert!(matches!(
        loop_plan.guard,
        rphp::vm::function::ScalarLongCallGuard::MethodCache { .. }
    ));
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);

    let method = globals
        .class_table
        .values()
        .find(|class| class.name.eq_ignore_ascii_case("ConditionalFloat"))
        .and_then(|class| {
            class
                .methods
                .iter()
                .find(|(name, ..)| name.eq_ignore_ascii_case("apply"))
        })
        .map(|(_, _, _, _, method)| method)
        .expect("conditional Double method");
    let leaf = method
        .scalar_double_plan
        .as_deref()
        .expect("conditional Double method leaf");
    assert!(leaf.select.is_some());
    assert!(!leaf.native_jit().is_compiled());
}

#[test]
fn typed_double_argument_expressions_enter_one_native_region() {
    let source = "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateFloat($i * 0.5, $scale + 1.0, 2.0); } echo $i . ':' . $total;";
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
    assert_eq!(captured_output(&output), "100000:5000350000");

    let loop_plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should compose Double argument expressions");
    assert_eq!(loop_plan.argument_program.operations.len(), 2);
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let leaf = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("calculateFloat"))
        .and_then(|(_, function)| function.scalar_double_plan.as_deref())
        .expect("Double leaf plan");
    assert!(!leaf.native_jit().is_compiled());
}

#[test]
fn nested_typed_double_leaf_is_flattened_into_one_native_region() {
    let source = "<?php function scaleAndShift(float $value, float $scale): float { return ($value * $scale) + 1.0; } function calculateNested(float $value, float $scale): float { return (scaleAndShift($value, $scale) * 0.5) + 2.0; } $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateNested($i * 0.5, $scale); } echo $i . ':' . $total;";
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
    assert_eq!(captured_output(&output), "100000:2500225000");

    let loop_plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the nested Double loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let outer = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("calculateNested"))
        .map(|(_, function)| function)
        .expect("compiled outer Double function");
    assert!(outer.scalar_double_plan.is_none());
    assert!(outer.composed_scalar_double_plan.is_some());
    assert_eq!(outer.common.call_count.get(), 100000);
    let inner = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("scaleAndShift"))
        .map(|(_, function)| function)
        .expect("compiled inner Double function");
    assert_eq!(inner.common.call_count.get(), 100000);
}
