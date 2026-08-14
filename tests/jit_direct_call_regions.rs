#![cfg(all(
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]

mod common;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::function::FunctionCommon;
use rphp::vm::planner::BlockPlan;

fn compile_and_execute(source: &str) -> (Vec<(String, rphp::vm::function::UserFunction)>, String) {
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
    for class_def in compilation.class_defs {
        globals.register_class(class_def).unwrap();
    }
    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    (functions, output)
}

fn closure_ops_plan<'a>(
    functions: &'a [(String, rphp::vm::function::UserFunction)],
    function_name: &str,
) -> &'a rphp::vm::quick::QuickLongOpsLoop {
    functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(function_name))
        .and_then(|(_, function)| {
            function
                .op_array
                .block_plans
                .iter()
                .find_map(|plan| match plan {
                    BlockPlan::QuickLongOps(plan) => Some(plan),
                    _ => None,
                })
        })
        .expect("compiler should select the closure service typed region")
}

#[test]
fn immutable_property_closure_enters_native_region_without_stale_reuse() {
    let (functions, output) = compile_and_execute(
        r#"<?php
final class Transform {
    public $callback;
    public function __construct($callback) { $this->callback = $callback; }
    public function apply($value) {
        $callback = $this->callback;
        return $callback($value);
    }
}
function runTransform($transform, $iterations) {
    $result = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }
    return $result;
}
$first = new Transform(function ($value) { return $value + 1; });
$second = new Transform(function ($value) { return $value + 2; });
echo runTransform($first, 100000), ':', runTransform($second, 100000);
"#,
    );
    assert_eq!(output, "100000:200000");

    let plan = closure_ops_plan(&functions, "runTransform");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn captured_property_closure_stays_on_canonical_boundary() {
    let (functions, output) = compile_and_execute(
        r#"<?php
final class Transform {
    public $callback;
    public function __construct($callback) { $this->callback = $callback; }
    public function apply($value) {
        $callback = $this->callback;
        return $callback($value);
    }
}
function runCapturedTransform($transform, $iterations) {
    $result = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }
    return $result;
}
$offset = 3;
$transform = new Transform(function ($value) use ($offset) { return $value + $offset; });
echo runCapturedTransform($transform, 100000);
"#,
    );
    assert_eq!(output, "300000");

    let plan = closure_ops_plan(&functions, "runCapturedTransform");
    assert_eq!(plan.native_jit().native_entries(), 0);
}

#[test]
fn indirect_closure_overflow_resumes_the_canonical_method_call() {
    let source = r#"<?php
final class Transform {
    public $callback;
    public function __construct($callback) { $this->callback = $callback; }
    public function apply($value) {
        $callback = $this->callback;
        return $callback($value);
    }
}
function runOverflowTransform($transform) {
    $result = 0;
    for ($index = 0; $index < 100; $index++) {
        $result = $transform->apply($index);
        if ($index === -1) { echo 'unreachable'; }
    }
    return $result;
}
$transform = new Transform(function ($value) {
    return ($value * 100000000000000000) % 7;
});
runOverflowTransform($transform);
"#;
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
    for class_def in compilation.class_defs {
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

    let plan = closure_ops_plan(&functions, "runOverflowTransform");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}
