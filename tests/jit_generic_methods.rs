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
use rphp::vm::quick::QuickLongOp;

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

fn generic_double_accumulate_plan<'a>(
    functions: &'a [(String, rphp::vm::function::UserFunction)],
    function_name: &str,
) -> &'a rphp::vm::quick::QuickDoubleCallAccumulateLoop {
    functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(function_name))
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
        .expect("compiler should select the generic Double method accumulate loop")
}

fn generic_mixed_ops_plan<'a>(
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
                    BlockPlan::QuickLongOps(plan)
                        if plan.ops.iter().any(|operation| {
                            matches!(operation, QuickLongOp::ObjectLongMethodCall { .. })
                        }) =>
                    {
                        Some(plan)
                    }
                    _ => None,
                })
        })
        .expect("compiler should select the generic mixed-method ops loop")
}

fn generic_property_ops_plan<'a>(
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
                    BlockPlan::QuickLongOps(plan)
                        if plan.ops.iter().any(|operation| {
                            matches!(operation, QuickLongOp::PropertyMethodCall { .. })
                        }) =>
                    {
                        Some(plan)
                    }
                    _ => None,
                })
        })
        .expect("compiler should select the generic property-method ops loop")
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

#[test]
fn exact_generic_double_tuple_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericDoubleJitBox<T> {
    public function scale(T $value, float $factor): T { return $value * $factor; }
}
function genericDoubleTotal($box) {
    $sum = 0.0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->scale(1.5, 2.0);
    }
    return $i . ':' . $sum;
}
echo genericDoubleTotal(new GenericDoubleJitBox::<float>());
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:300000");

    let plan = generic_double_accumulate_plan(&functions, "genericDoubleTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_generic_double_tuple_mismatch_stays_on_canonical_boundary() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericDoubleJitBox<T> {
    public function scale(T $value, float $factor): T { return $value * $factor; }
}
function genericDoubleTotal($box) {
    $sum = 0.0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->scale(1.5, 2.0);
    }
    return $sum;
}
echo genericDoubleTotal(new GenericDoubleJitBox::<float>()) . '|';
genericDoubleTotal(new GenericDoubleJitBox::<string>());
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "300000|");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to GenericDoubleJitBox::scale()"),
        "{rendered}"
    );
    assert!(rendered.contains("reified class type"), "{rendered}");

    let plan = generic_double_accumulate_plan(&functions, "genericDoubleTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_generic_double_tuple_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class NestedGenericDoubleJitBox<T> {
    public function scale(T $value, float $factor): T { return $value * $factor; }
    public function composed(float $value, float $factor): float {
        return $this->scale($value, $factor) + 1.0;
    }
}
function nestedGenericDoubleTotal($box) {
    $sum = 0.0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->composed(1.5, 2.0);
    }
    return $i . ':' . $sum;
}
echo nestedGenericDoubleTotal(new NestedGenericDoubleJitBox::<float>());
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:400000");

    let plan = generic_double_accumulate_plan(&functions, "nestedGenericDoubleTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn nested_reified_double_tuple_mismatch_stays_on_canonical_boundary() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class NestedGenericDoubleJitBox<T> {
    public function scale(T $value, float $factor): T { return $value * $factor; }
    public function composed(float $value, float $factor): float {
        return $this->scale($value, $factor) + 1.0;
    }
}
function nestedGenericDoubleTotal($box) {
    $sum = 0.0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->composed(1.5, 2.0);
    }
    return $sum;
}
echo nestedGenericDoubleTotal(new NestedGenericDoubleJitBox::<float>()) . '|';
nestedGenericDoubleTotal(new NestedGenericDoubleJitBox::<string>());
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "400000|");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to NestedGenericDoubleJitBox::scale()"),
        "{rendered}"
    );
    assert!(rendered.contains("reified class type"), "{rendered}");

    let plan = generic_double_accumulate_plan(&functions, "nestedGenericDoubleTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn composed_object_generic_long_tuple_uses_quick_body() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class ComposedGenericLongBox<T> {
    public function step(T $value): T { return $value + 1; }
}
function consumeComposedLong(ComposedGenericLongBox $box, int $value): int {
    return ($box->step($value) % 97) ^ 13;
}
function composedGenericLongTotal($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += consumeComposedLong($box, $i);
    }
    return $i . ':' . $sum;
}
echo composedGenericLongTotal(new ComposedGenericLongBox::<int>());
"#,
    );
    result.unwrap();
    assert!(output.starts_with("100000:"), "{output}");

    let plan = generic_accumulate_plan(&functions, "composedGenericLongTotal");
    assert!(matches!(
        plan.term,
        rphp::vm::quick::QuickLongTerm::ScalarFunctionCall { .. }
    ));
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn composed_object_reified_long_mismatch_replays_canonical_boundary() {
    let (_, _, result, output) = compile_and_execute(
        r#"<?php
class ComposedGenericLongBox<T> {
    public function step(T $value): T { return $value + 1; }
}
function consumeComposedLong(ComposedGenericLongBox $box, int $value): int {
    return ($box->step($value) % 97) ^ 13;
}
function composedGenericLongTotal($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += consumeComposedLong($box, $i);
    }
    return $sum;
}
echo composedGenericLongTotal(new ComposedGenericLongBox::<int>()) . '|';
composedGenericLongTotal(new ComposedGenericLongBox::<string>());
"#,
    );
    let error = result.unwrap_err();
    assert!(output.ends_with('|'), "{output}");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to ComposedGenericLongBox::step()"),
        "{rendered}"
    );
    assert!(rendered.contains("reified class type"), "{rendered}");
}

#[test]
fn composed_object_generic_string_tuple_uses_quick_body() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class ComposedGenericStringBox<T> {
    public function label(T $value): string {
        if (($value & 1) === 0) { return 'even'; }
        return 'odd';
    }
}
function consumeComposedString(ComposedGenericStringBox $box, int $value): int {
    return strlen($box->label($value));
}
function composedGenericStringTotal($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += consumeComposedString($box, $i);
    }
    return $i . ':' . $sum;
}
echo composedGenericStringTotal(new ComposedGenericStringBox::<int>());
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:350000");

    let plan = generic_accumulate_plan(&functions, "composedGenericStringTotal");
    assert!(matches!(
        plan.term,
        rphp::vm::quick::QuickLongTerm::ScalarFunctionCall { .. }
    ));
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn composed_object_reified_string_mismatch_replays_canonical_boundary() {
    let (_, _, result, output) = compile_and_execute(
        r#"<?php
class ComposedGenericStringBox<T> {
    public function label(T $value): string {
        if (($value & 1) === 0) { return 'even'; }
        return 'odd';
    }
}
function consumeComposedString(ComposedGenericStringBox $box, int $value): int {
    return strlen($box->label($value));
}
function composedGenericStringTotal($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += consumeComposedString($box, $i);
    }
    return $sum;
}
echo composedGenericStringTotal(new ComposedGenericStringBox::<int>()) . '|';
composedGenericStringTotal(new ComposedGenericStringBox::<string>());
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "350000|");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to ComposedGenericStringBox::label()"),
        "{rendered}"
    );
    assert!(rendered.contains("reified class type"), "{rendered}");
}

#[test]
fn exact_generic_mixed_tuple_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericMixedJitBox<T> {
    public function score(int $base, T $route): int {
        return $base + strlen($route);
    }
}
function genericMixedTotal($box) {
    $values = ['left' => 0, 'right' => 0];
    $route = 'left';
    $needle = -1;
    for ($i = 0; $i < 100000; $i++) {
        if (($i % 2) == 0) { $route = 'right'; } else { $route = 'left'; }
        $score = $box->score($i, $route);
        $values[$route] = $values[$route] + $score;
        if ($i === $needle) { echo 'never'; }
    }
    return $values['left'] . ':' . $values['right'] . ':' . $i;
}
echo genericMixedTotal(new GenericMixedJitBox::<string>());
"#,
    );
    result.unwrap();
    assert_eq!(output, "2500200000:2500200000:100000");

    let plan = generic_mixed_ops_plan(&functions, "genericMixedTotal");
    assert!(
        plan.native_jit().is_straight_compiled(),
        "ops: {:?}",
        plan.ops
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_generic_mixed_tuple_mismatch_replays_canonical_boundary() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericMixedJitBox<T> {
    public function score(int $base, T $route): int {
        return $base + strlen($route);
    }
}
function genericMixedTotal($box) {
    $values = ['left' => 0, 'right' => 0];
    $route = 'left';
    $needle = -1;
    for ($i = 0; $i < 100000; $i++) {
        if (($i % 2) == 0) { $route = 'right'; } else { $route = 'left'; }
        $score = $box->score($i, $route);
        $values[$route] = $values[$route] + $score;
        if ($i === $needle) { echo 'never'; }
    }
    return $values['left'] . ':' . $values['right'] . ':' . $i;
}
echo genericMixedTotal(new GenericMixedJitBox::<string>()) . '|';
genericMixedTotal(new GenericMixedJitBox::<int>());
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "2500200000:2500200000:100000|");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #2 passed to GenericMixedJitBox::score()"),
        "{rendered}"
    );
    assert!(rendered.contains("reified class type"), "{rendered}");

    let plan = generic_mixed_ops_plan(&functions, "genericMixedTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn bound_generic_property_mutator_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class BoundGenericPropertyJitBox<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function add(T $value): void { $this->total = $this->total + $value; }
}
function genericPropertyTotal($box) {
    $checksum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $box->add(1);
        $checksum += $i;
    }
    return $i . ':' . $box->total . ':' . $checksum;
}
echo genericPropertyTotal(new BoundGenericPropertyJitBox::<int>(0));
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:100000:4999950000");

    let plan = generic_property_ops_plan(&functions, "genericPropertyTotal");
    assert!(
        plan.native_jit().is_straight_compiled(),
        "object mask: {}, ops: {:?}",
        plan.object_input_mask,
        plan.ops,
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_property_mismatch_replays_canonical_store_boundary() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericPropertyJitBox<T> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function set(int $value): void { $this->value = $value; }
}
function genericPropertySet($box) {
    $checksum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $box->set($i);
        $checksum += $i;
    }
    return $box->value . ':' . $checksum;
}
echo genericPropertySet(new GenericPropertyJitBox::<int>(0)) . '|';
genericPropertySet(new GenericPropertyJitBox::<string>('seed'));
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "99999:4999950000|");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Value does not match reified property GenericPropertyJitBox::$value"),
        "{rendered}"
    );

    let plan = generic_property_ops_plan(&functions, "genericPropertySet");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn generic_void_property_mutator_with_value_return_stays_canonical() {
    let (_, _, result, output) = compile_and_execute(
        r#"<?php
class InvalidGenericVoidMutator<T : int> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function update(T $value): void {
        $this->value = $value;
        return 1;
    }
}
$box = new InvalidGenericVoidMutator::<int>(0);
$box->update(1);
echo $box->value;
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("A void function must not return a value")
            || rendered.contains(
                "Return value of InvalidGenericVoidMutator::update() does not match its reified class type",
            ),
        "{rendered}"
    );
}
