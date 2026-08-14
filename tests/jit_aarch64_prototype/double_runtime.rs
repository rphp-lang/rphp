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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "576:7"
    );

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
    let source = "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } function runTypedDouble() { $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateFloat(1.5, 2.5, $scale); } echo $i . ':' . $total; } runTypedDouble();";
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
        "100000:600000"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runTypedDouble"))
        .map(|(_, function)| function)
        .unwrap()
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
    let source = "<?php function conditionalFloat(float $value, float $pivot): float { if ($value < $pivot) { return ($value * 1.5) + 2.0; } return ($value * 0.5) - 1.0; } function runConditionalDouble() { $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += conditionalFloat($i * 0.5, 25000.0); } echo $i . ':' . $total; } runConditionalDouble();";
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
        "100000:1875025000"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runConditionalDouble"))
        .map(|(_, function)| function)
        .unwrap()
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
    let source = "<?php class FloatCalculator { public function calculate(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } } function runDoubleMethod() { $calculator = new FloatCalculator(); $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += $calculator->calculate(1.5, 2.5, 2.0); } echo $i . ':' . $total; } runDoubleMethod();";
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:600000"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runDoubleMethod"))
        .map(|(_, function)| function)
        .unwrap()
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
    let source = "<?php class ConditionalFloat { public function apply(float $value, float $pivot): float { $scaled = $value * 1.0; if ($scaled < $pivot) { $result = ($scaled * 1.5) + 2.0; return $result; } $result = ($scaled * 0.5) - 1.0; return $result; } } function runConditionalDoubleMethod() { $calculator = new ConditionalFloat(); $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += $calculator->apply($i * 0.5, 25000.0); } echo $i . ':' . $total; } runConditionalDoubleMethod();";
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:1875025000"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runConditionalDoubleMethod"))
        .map(|(_, function)| function)
        .unwrap()
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
    let source = "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } function runDoubleExpressions() { $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateFloat($i * 0.5, $scale + 1.0, 2.0); } echo $i . ':' . $total; } runDoubleExpressions();";
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
        "100000:5000350000"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runDoubleExpressions"))
        .map(|(_, function)| function)
        .unwrap()
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
    let source = "<?php function scaleAndShift(float $value, float $scale): float { return ($value * $scale) + 1.0; } function calculateNested(float $value, float $scale): float { return (scaleAndShift($value, $scale) * 0.5) + 2.0; } function runNestedDouble() { $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateNested($i * 0.5, $scale); } echo $i . ':' . $total; } runNestedDouble();";
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
        "100000:2500225000"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runNestedDouble"))
        .map(|(_, function)| function)
        .unwrap()
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

#[test]
fn composed_conditional_double_leaf_is_flattened_into_one_native_region() {
    let source = "<?php function conditionalFloat(float $value, float $pivot): float { if ($value < $pivot) { return ($value * 1.5) + 2.0; } return ($value * 0.5) - 1.0; } function composedFloat(float $value, float $pivot): float { return (conditionalFloat($value, $pivot) * 1.25) + 3.0; } function runComposedDouble() { $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += composedFloat($i * 0.5, 25000.0); } echo $i . ':' . $total; } runComposedDouble();";
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
        "100000:2344081250"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runComposedDouble"))
        .map(|(_, function)| function)
        .unwrap()
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the composed conditional Double loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let outer = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("composedFloat"))
        .map(|(_, function)| function)
        .expect("compiled composed Double function");
    assert!(outer.scalar_double_plan.is_none());
    assert!(outer.composed_scalar_double_plan.is_some());
    let leaf = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("conditionalFloat"))
        .and_then(|(_, function)| function.scalar_double_plan.as_deref())
        .expect("conditional Double leaf");
    assert!(leaf.select.is_some());
}

#[test]
fn same_receiver_conditional_double_method_is_flattened_into_one_native_region() {
    let source = "<?php class FloatPipeline { public function conditional(float $value, float $pivot): float { if ($value < $pivot) { return ($value * 1.5) + 2.0; } return ($value * 0.5) - 1.0; } public function composed(float $value, float $pivot): float { return ($this->conditional($value, $pivot) * 1.25) + 3.0; } } function runConditionalPipeline() { $pipeline = new FloatPipeline(); $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += $pipeline->composed($i * 0.5, 25000.0); } echo $i . ':' . $total; } runConditionalPipeline();";
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:2344081250"
    );
    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runConditionalPipeline"))
        .map(|(_, function)| function)
        .unwrap()
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the composed conditional method loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let class = globals
        .class_table
        .values()
        .find(|class| class.name.eq_ignore_ascii_case("FloatPipeline"))
        .expect("registered FloatPipeline");
    let method = |name: &str| {
        class
            .methods
            .iter()
            .find(|(candidate, ..)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, _, _, _, method)| method)
            .expect("compiled FloatPipeline method")
    };
    assert!(
        method("conditional")
            .scalar_double_plan
            .as_deref()
            .and_then(|plan| plan.select)
            .is_some()
    );
    assert!(method("composed").composed_scalar_double_plan.is_some());
}

#[test]
fn same_receiver_nested_double_method_is_flattened_into_one_native_region() {
    let source = "<?php class FloatPipeline { public function scaleAndShift(float $value, float $scale): float { return ($value * $scale) + 1.0; } public function calculate(float $value, float $scale): float { return ($this->scaleAndShift($value, $scale) * 0.5) + 2.0; } } function runNestedPipeline() { $pipeline = new FloatPipeline(); $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += $pipeline->calculate($i * 0.5, $scale); } echo $i . ':' . $total; } runNestedPipeline();";
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:2500225000"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runNestedPipeline"))
        .map(|(_, function)| function)
        .unwrap()
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the same-receiver Double method loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    let class = globals
        .class_table
        .values()
        .find(|class| class.name.eq_ignore_ascii_case("FloatPipeline"))
        .expect("registered FloatPipeline");
    let method = |name: &str| {
        class
            .methods
            .iter()
            .find(|(candidate, ..)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, _, _, _, method)| method)
            .expect("compiled FloatPipeline method")
    };
    let outer = method("calculate");
    assert!(outer.scalar_double_plan.is_none());
    let composed = outer
        .composed_scalar_double_plan
        .as_deref()
        .expect("same-receiver composed Double plan");
    assert!(composed.operations.iter().any(|operation| matches!(
        operation,
        rphp::vm::function::ComposedScalarDoubleOp::Call(call)
            if matches!(call.guard, rphp::vm::function::ScalarLongCallGuard::MethodCache {
                receiver_slot: 0,
                ..
            })
    )));
    assert_eq!(outer.common.call_count.get(), 100000);
    assert_eq!(method("scaleAndShift").common.call_count.get(), 100000);
}

#[test]
fn recursive_composed_double_tree_is_flattened_into_one_native_region() {
    let source = "<?php function scaleAndShift(float $value, float $scale): float { return ($value * $scale) + 1.0; } function calculateNested(float $value, float $scale): float { return (scaleAndShift($value, $scale) * 0.5) + 2.0; } function calculateOuter(float $value, float $scale): float { return calculateNested($value, $scale) + 3.0; } function runRecursiveDouble() { $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += calculateOuter($i * 0.5, $scale); } echo $i . ':' . $total; } runRecursiveDouble();";
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
        "100000:2500525000"
    );

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runRecursiveDouble"))
        .map(|(_, function)| function)
        .unwrap()
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the recursively composed Double loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 0);

    for name in ["calculateOuter", "calculateNested", "scaleAndShift"] {
        let function = functions
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, function)| function)
            .expect("compiled Double call-tree function");
        assert_eq!(function.common.call_count.get(), 100000, "{name}");
    }
}

#[test]
fn monomorphic_float_method_uses_class_cache_and_double_jit() {
    let call_count = usize::from(SCALAR_DOUBLE_JIT_HOT_THRESHOLD) + 8;
    let mut source = String::from(
        "<?php class FloatModel { public function blend(float $a, float $b, float $c): float { return (($a + 1.5) * $b) / $c; } } function callBlend($model): float { return $model->blend(2.5, 4.0, 2.0); } $model = new FloatModel(); $total = 0.0;",
    );
    for _ in 0..call_count {
        source.push_str("$total = $total + callBlend($model);");
    }
    source.push_str("echo $total;");
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
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "576"
    );

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
fn nested_double_method_loop_zero_divisor_replays_canonical_php_error() {
    let source = "<?php class Divider { public function divide(float $value, float $divisor): float { return $value / $divisor; } public function calculate(float $value, float $divisor): float { return $this->divide($value, $divisor) + 1.0; } } function accumulate(Divider $divider): float { $total = 0.0; for ($i = 0; $i < 100000; $i++) { $total += $divider->calculate(8.0, 50000.0 - ($i * 1.0)); } return $total; } $divider = new Divider(); accumulate($divider);";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let class_defs = compilation.class_defs;
    let (mut globals, _output) = common::make_eg_with_capture();
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in class_defs {
        globals.register_class(class_def).unwrap();
    }

    let error = execute::execute(&mut globals, &main).unwrap_err();
    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message) if message == "Division by zero"
    ));

    let loop_plan = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("accumulate"))
        .and_then(|(_, function)| {
            function
                .op_array
                .block_plans
                .iter()
                .find_map(|plan| match plan {
                    BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                    _ => None,
                })
        })
        .expect("nested method Double loop");
    assert!(loop_plan.native_jit().is_compiled());
    assert_eq!(loop_plan.native_jit().native_entries(), 1);
    assert_eq!(loop_plan.native_jit().side_exits(), 1);
}
