#![cfg(all(
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    ),
    any(feature = "php-generics-erased", feature = "php-generics-reified")
))]

mod common;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::function::FunctionCommon;
use rphp::vm::planner::BlockPlan;

fn compile_and_execute(
    source: &str,
) -> (
    rphp::vm::function::UserFunction,
    Vec<(String, rphp::vm::function::UserFunction)>,
    Result<rphp::value::Value, execute::VmError>,
    String,
) {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let functions = compilation.functions;
    let (mut globals, output) = common::make_eg_with_capture();
    globals.generic_metadata = compilation.generic_metadata;
    for (name, function) in &functions {
        globals
            .register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class_def in compilation.class_defs {
        globals.register_class(class_def).unwrap();
    }
    let result = execute::execute(&mut globals, &main);
    drop(globals);
    let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    (main, functions, result, output)
}

fn generic_accumulate_plan<'a>(
    functions: &'a [(String, rphp::vm::function::UserFunction)],
    function_name: &str,
) -> &'a rphp::vm::quick::QuickLongAccumulateLoop {
    functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(function_name))
        .and_then(|(_, function)| {
            function
                .op_array
                .block_plans
                .iter()
                .find_map(|plan| match plan {
                    BlockPlan::QuickLongAccumulate(plan) => Some(plan),
                    _ => None,
                })
        })
        .expect("compiler should select the generic method accumulate loop")
}

#[test]
fn exact_generic_long_tuple_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericJitBox<T> {
    public function step(T $value): T { return $value + 1; }
}
function genericTotal($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->step($i);
    }
    return $i . ':' . $sum;
}
echo genericTotal(new GenericJitBox::<int>());
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:5000050000");

    let plan = generic_accumulate_plan(&functions, "genericTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_generic_long_tuple_mismatch_stays_on_canonical_boundary() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericJitBox<T> {
    public function step(T $value): T { return $value + 1; }
}
function genericTotal($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->step($i);
    }
    return $sum;
}
echo genericTotal(new GenericJitBox::<int>()) . '|';
genericTotal(new GenericJitBox::<string>());
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "5000050000|");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to GenericJitBox::step()"),
        "{rendered}"
    );
    assert!(rendered.contains("reified class type"), "{rendered}");

    let plan = generic_accumulate_plan(&functions, "genericTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_generic_long_tuple_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
function genericJitAdd(int $left, int $right): int { return $left + $right; }
class NestedGenericJitBox<T> {
    public function multiply(T $left, T $right): T { return $left * $right; }
}
function nestedGenericTotal($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += genericJitAdd($i, $box->multiply($i, 2));
    }
    return $i . ':' . $sum;
}
echo nestedGenericTotal(new NestedGenericJitBox::<int>());
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:14999850000");

    let plan = generic_accumulate_plan(&functions, "nestedGenericTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn nested_reified_tuple_mismatch_stays_on_canonical_boundary() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
function genericJitAdd(int $left, int $right): int { return $left + $right; }
class NestedGenericJitBox<T> {
    public function multiply(T $left, T $right): T { return $left * $right; }
}
function nestedGenericTotal($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += genericJitAdd($i, $box->multiply($i, 2));
    }
    return $sum;
}
echo nestedGenericTotal(new NestedGenericJitBox::<int>()) . '|';
nestedGenericTotal(new NestedGenericJitBox::<string>());
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "14999850000|");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to NestedGenericJitBox::multiply()"),
        "{rendered}"
    );
    assert!(rendered.contains("reified class type"), "{rendered}");

    let plan = generic_accumulate_plan(&functions, "nestedGenericTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}
