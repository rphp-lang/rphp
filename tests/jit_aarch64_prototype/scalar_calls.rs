#[test]
fn real_php_scalar_function_enters_native_accumulate_region() {
    let source = "<?php function calculateNative(int $value): int { return ($value * 2) + 1; } $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += calculateNative($i); } echo $i . ':' . $sum;";
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
        .expect("compiler should select a scalar-function accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_scalar_functions_enter_one_native_accumulate_region() {
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
        .expect("compiler should select a nested scalar-function accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn mixed_function_method_tree_enters_one_native_accumulate_region() {
    let source = "<?php class NativeMultiplier { public function mul(int $left, int $right): int { return $left * $right; } } function addNative(int $left, int $right): int { return $left + $right; } $math = new NativeMultiplier(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += addNative($i, $math->mul($i, 2)); } echo $i . ':' . $sum;";
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
        .expect("compiler should select a mixed scalar-call accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn native_scalar_function_overflow_resumes_canonical_root_call() {
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
        .expect("runFunctionOverflow should use a scalar-function accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}
#[test]
fn real_php_nested_scalar_methods_enter_one_native_accumulate_region() {
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
        .expect("compiler should select a nested scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_scalar_methods_lower_checked_caller_argument_expressions() {
    let source = "<?php class ExpressionTree { public function add($left, $right) { return $left + $right; } public function mul($left, $right) { return $left * $right; } } $math = new ExpressionTree(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $math->add($i + 1, $math->mul($i, 2)); } echo $i . ':' . $sum;";
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
        .expect("compiler should select a scalar argument-expression loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn conditional_scalar_method_enters_native_accumulate_region() {
    let source = "<?php class ConditionalKernel { public function route(int $value): int { if (($value & 1) == 0) { return ($value * 3) + 1; } return ($value * 5) - 2; } } $kernel = new ConditionalKernel(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $kernel->route($i); } echo $i . ':' . $sum;";
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
        "100000:19999800000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a conditional scalar-method loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_conditional_scalar_method_flattens_with_outer_method() {
    let source = "<?php class ConditionalTree { public function add(int $left, int $right): int { return $left + $right; } public function route(int $value): int { if (($value & 1) == 0) { return $value * 2; } return $value + 3; } } $tree = new ConditionalTree(); $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $tree->add($i, $tree->route($i)); } echo $i . ':' . $sum;";
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
        "100000:12500000000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a nested conditional scalar loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn conditional_scalar_method_skips_overflow_in_inactive_arm() {
    let source = "<?php class InactiveOverflowKernel { public function choose(int $value): int { if ($value < 100) { return $value + 1; } return ($value * 100000000000000000) % 7; } } $kernel = new InactiveOverflowKernel(); $sum = 0; for ($i = 0; $i < 80; $i++) { $sum += $kernel->choose($i); } echo $i . ':' . $sum;";
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
        "80:3240"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select the inactive-overflow scalar loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn conditional_scalar_method_selected_overflow_replays_canonical_call() {
    let source = "<?php class SelectedOverflowKernel { public function choose(int $value): int { if ($value < 90) { return $value + 1; } return ($value * 100000000000000000) % 7; } } function runSelectedOverflow(): int { $kernel = new SelectedOverflowKernel(); $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += $kernel->choose($i); } return $sum; } runSelectedOverflow();";
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
        .find(|(name, _)| name.eq_ignore_ascii_case("runSelectedOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runSelectedOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runSelectedOverflow should use a conditional scalar loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn conditional_scalar_method_rejects_polymorphic_target() {
    let source = "<?php class FirstConditional { public function route(int $value): int { if (($value & 1) == 0) { return $value + 1; } return $value + 2; } } class SecondConditional { public function route(int $value): int { if (($value & 1) == 0) { return $value + 3; } return $value + 4; } } function runConditional($kernel): int { $sum = 0; for ($i = 0; $i < 1000; $i++) { $sum += $kernel->route($i); } return $sum; } echo runConditional(new FirstConditional()) . ':' . runConditional(new SecondConditional());";
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
        "501000:503000"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runConditional"))
        .map(|(_, function)| function)
        .expect("compiled runConditional function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runConditional should use a conditional scalar-method loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn nested_scalar_method_guard_rejects_changed_inner_target() {
    let source = "<?php class OuterMath { public function add($left, $right) { return $left + $right; } } class DoubleMath { public function mul($left, $right) { return $left * $right; } } class TripleMath { public function mul($left, $right) { return $left * ($right + 1); } } function runTree($outer, $inner): int { $sum = 0; for ($i = 0; $i < 1000; $i++) { $sum += $outer->add($i, $inner->mul($i, 2)); } return $sum; } $outer = new OuterMath(); echo runTree($outer, new DoubleMath()) . ':' . runTree($outer, new TripleMath());";
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
        "1498500:1998000"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runTree"))
        .map(|(_, function)| function)
        .expect("compiled runTree function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runTree should use a nested scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn nested_scalar_method_overflow_replays_the_root_call_tree() {
    let source = "<?php class OuterOverflow { public function add($left, $right) { return $left + $right; } } class InnerOverflow { public function transform($value) { return ($value * 100000000000000000) % 7; } } function runNestedOverflow(): int { $outer = new OuterOverflow(); $inner = new InnerOverflow(); $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += $outer->add($i, $inner->transform($i)); } return $sum; } runNestedOverflow();";
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
        .find(|(name, _)| name.eq_ignore_ascii_case("runNestedOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runNestedOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runNestedOverflow should use a nested scalar-method loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_scalar_method_guard_rejects_polymorphic_target() {
    let source = "<?php class FirstKernel { public function transform(int $value): int { return $value + 1; } } class SecondKernel { public function transform(int $value): int { return $value + 2; } } function runKernel($kernel): int { $sum = 0; for ($i = 0; $i < 1000; $i++) { $sum += $kernel->transform($i); } return $sum; } echo runKernel(new FirstKernel()) . ':' . runKernel(new SecondKernel());";
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
        "500500:501500"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runKernel"))
        .map(|(_, function)| function)
        .expect("compiled runKernel function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runKernel should use a scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn native_scalar_method_overflow_resumes_canonical_call() {
    let source = "<?php class OverflowKernel { public function transform(int $value): int { return ($value * 100000000000000000) % 7; } } function runOverflow(): int { $kernel = new OverflowKernel(); $sum = 0; for ($i = 0; $i < 100; $i++) { $sum += $kernel->transform($i); } return $sum; } try { runOverflow(); } catch (TypeError $error) { echo 'caught'; }";
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
        .find(|(name, _)| name.eq_ignore_ascii_case("runOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runOverflow should use a scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_scalar_method_sum_overflow_resumes_canonical_add() {
    let source = "<?php class SumKernel { public function transform(int $value): int { return $value + 1; } } function runSumOverflow(): int { $kernel = new SumKernel(); $sum = PHP_INT_MAX - 100000; for ($i = 0; $i < 1000; $i++) { $sum += $kernel->transform($i); } return $sum; } try { runSumOverflow(); } catch (TypeError $error) { echo 'caught'; }";
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
        "caught"
    );

    let function = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runSumOverflow"))
        .map(|(_, function)| function)
        .expect("compiled runSumOverflow function");
    let plan = function
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("runSumOverflow should use a scalar-method accumulate loop");
    assert!(plan.native_jit().is_call_compiled());
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn real_php_constant_term_loop_enters_specialized_native_region() {
    let source =
        "<?php $sum = 0; for ($i = 0; $i < 100000; $i++) { $sum += $i + 1; } echo $i . ':' . $sum;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    assert_eq!(
        String::from_utf8(output.lock().unwrap().clone()).unwrap(),
        "100000:5000050000"
    );

    let plan = main
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("compiler should select a constant-term accumulate loop");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(
        plan.native_jit().range_proven_chunks(),
        plan.native_jit().native_chunks()
    );
    assert_eq!(plan.native_jit().range_proof_evaluations(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn native_loop_sum_overflow_resumes_canonical_php_instruction() {
    let source = "<?php function overflow(): int { $sum = PHP_INT_MAX - 1000; for ($i = 0; $i < 60; $i++) { $sum += $i; } return $sum; } try { overflow(); } catch (TypeError $error) { echo 'caught'; }";
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

    let overflow = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("overflow"))
        .map(|(_, function)| function)
        .expect("compiled overflow function");
    let plan = overflow
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("overflow function should have an accumulate plan");
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_calls(), 1);
    assert_eq!(plan.native_jit().range_proven_chunks(), 0);
    assert_eq!(plan.native_jit().range_proof_evaluations(), 2);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn native_constant_term_overflow_resumes_canonical_term_instruction() {
    let source = "<?php function plusTwo(int $start, int $bound): int { $sum = 0; for ($i = $start; $i < $bound; $i++) { $sum += $i + 2; } return $sum; } plusTwo(0, 100); try { plusTwo(PHP_INT_MAX - 2, PHP_INT_MAX); } catch (TypeError $error) { echo 'caught'; }";
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

    let plus_two = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("plusTwo"))
        .map(|(_, function)| function)
        .expect("compiled plusTwo function");
    let plan = plus_two
        .op_array
        .block_plans
        .iter()
        .find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan),
            _ => None,
        })
        .expect("plusTwo should have a constant-term accumulate plan");
    assert!(plan.native_jit().is_compiled());
    assert!(plan.native_jit().native_entries() >= 2);
    assert_eq!(plan.native_jit().native_calls(), 2);
    assert!(plan.native_jit().range_proven_chunks() >= 1);
    assert!(plan.native_jit().range_proven_chunks() < plan.native_jit().native_chunks());
    assert_eq!(plan.native_jit().range_proof_evaluations(), 3);
    assert_eq!(plan.native_jit().side_exits(), 1);
}
