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
fn immutable_captured_property_closure_enters_native_region() {
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
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn same_declaration_capture_change_never_reuses_stale_native_constants() {
    let (functions, output) = compile_and_execute(
        r#"<?php
final class Transform {
    public $callback;
    public function __construct($callback) { $this->callback = $callback; }
    public function apply(int $value): int {
        $callback = $this->callback;
        return $callback($value);
    }
}
function makeTransform(int $offset, string $prefix): Transform {
    return new Transform(static function (int $value) use ($offset, $prefix): int {
        return $value + $offset + strlen($prefix);
    });
}
function runCapturedTransform(Transform $transform, int $iterations): int {
    $result = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }
    return $result;
}
$first = makeTransform(2, 'a');
$second = makeTransform(7, 'four');
echo runCapturedTransform($first, 100000), ':', runCapturedTransform($second, 100000);
"#,
    );
    assert_eq!(output, "300000:1100000");
    assert_eq!(
        functions
            .iter()
            .filter(|(name, _)| name.starts_with("__closure_"))
            .count(),
        1,
        "both runtime Closure instances must share one compiled declaration"
    );

    let plan = closure_ops_plan(&functions, "runCapturedTransform");
    assert_eq!(
        plan.native_jit().native_entries(),
        1,
        "the first capture configuration should enter native code and the changed constants should use the canonical lane"
    );
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn reference_captured_property_closure_stays_on_canonical_boundary() {
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
function runReferenceTransform($transform, $iterations) {
    $result = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }
    return $result;
}
$offset = 3;
$transform = new Transform(function ($value) use (&$offset) { return $value + $offset; });
echo runReferenceTransform($transform, 100000);
"#,
    );
    assert_eq!(output, "300000");

    let plan = closure_ops_plan(&functions, "runReferenceTransform");
    assert_eq!(plan.native_jit().native_entries(), 0);
}

#[test]
fn captured_argument_closure_and_dead_alias_enter_one_native_region() {
    let (functions, output) = compile_and_execute(
        r#"<?php
function invokeCaptured(Closure $callback, int $value): int {
    return $callback($value);
}
function runCapturedArgument(int $iterations): int {
    $prefix = 'kept';
    $offset = 7;
    $callback = static function (int $value) use ($prefix, $offset): int {
        return strlen($prefix) + $offset + $value;
    };
    $sum = 0;
    for ($index = 0; $index < $iterations; ++$index) {
        $copy = $callback;
        $sum += invokeCaptured($copy, $index & 255);
    }
    return $sum;
}
echo runCapturedArgument(100000);
"#,
    );
    assert_eq!(output, "13842320");

    let plan = closure_ops_plan(&functions, "runCapturedArgument");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn by_reference_closure_leaf_and_wrapper_stay_canonical() {
    let (functions, output) = compile_and_execute(
        r#"<?php
function &invokeReference(Closure $callback, int $value): int {
    return $callback($value);
}
function &incrementReference(int $value): int {
    return $value + 1;
}
function runReferenceWrapper(Closure $callback, int $iterations): int {
    $sum = 0;
    for ($index = 0; $index < $iterations; $index++) {
        $sum += invokeReference($callback, $index);
    }
    return $sum;
}
$seed = 5;
$callback = static function &(int $ignored) use ($seed): int {
    return $seed;
};
echo runReferenceWrapper($callback, 100000);
"#,
    );
    assert_eq!(output, "500000");

    let wrapper = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("invokeReference"))
        .map(|(_, function)| function)
        .expect("reference wrapper should be compiled");
    assert!(wrapper.common.sig.returns_reference);
    assert!(wrapper.scalar_long_plan.is_none());
    assert!(wrapper.indirect_scalar_long_plan().is_none());

    let no_capture_leaf = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("incrementReference"))
        .map(|(_, function)| function)
        .expect("no-capture reference leaf should be compiled");
    assert!(no_capture_leaf.common.sig.returns_reference);
    assert!(no_capture_leaf.scalar_long_plan.is_none());

    let closure = functions
        .iter()
        .find(|(name, _)| name.starts_with("__closure_"))
        .map(|(_, function)| function)
        .expect("reference-returning closure should be compiled");
    assert!(closure.common.sig.returns_reference);
    assert!(closure.captured_typed_long_plan().is_none());

    let outer = functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runReferenceWrapper"))
        .map(|(_, function)| function)
        .expect("outer reference wrapper loop should be compiled");
    assert!(
        !outer
            .op_array
            .block_plans
            .iter()
            .any(|plan| matches!(plan, BlockPlan::QuickLongOps(_))),
        "a by-reference leaf must prevent native outer-region planning"
    );
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

#[test]
fn captured_argument_overflow_replays_from_the_virtual_alias() {
    let source = r#"<?php
function invokeCapturedOverflow(Closure $callback, int $value): int {
    return $callback($value);
}
function runCapturedOverflow() {
    $offset = 1;
    $callback = static function (int $value) use ($offset): int {
        return (($value * 100000000000000000) + $offset) % 7;
    };
    $result = 0;
    for ($index = 0; $index < 100; $index++) {
        $copy = $callback;
        $result = invokeCapturedOverflow($copy, $index);
        if ($index === -1) { echo 'unreachable'; }
    }
    return $result;
}
runCapturedOverflow();
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

    let error = execute::execute(&mut globals, &main).unwrap_err();
    drop(globals);
    assert!(matches!(
        error,
        execute::VmError::Fatal(message)
            if message == "Unsupported operand types for %"
    ));
    assert!(output.lock().unwrap().is_empty());

    let plan = closure_ops_plan(&functions, "runCapturedOverflow");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}
