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

fn generic_ops_plan<'a>(
    functions: &'a [(String, rphp::vm::function::UserFunction)],
    function_name: &str,
    contains: fn(&QuickLongOp) -> bool,
    expectation: &str,
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
                    BlockPlan::QuickLongOps(plan) if plan.ops.iter().any(contains) => Some(plan),
                    _ => None,
                })
        })
        .unwrap_or_else(|| panic!("{expectation}"))
}

fn generic_mixed_ops_plan<'a>(
    functions: &'a [(String, rphp::vm::function::UserFunction)],
    function_name: &str,
) -> &'a rphp::vm::quick::QuickLongOpsLoop {
    generic_ops_plan(
        functions,
        function_name,
        |operation| matches!(operation, QuickLongOp::ObjectLongMethodCall { .. }),
        "compiler should select the generic mixed-method ops loop",
    )
}

fn generic_property_ops_plan<'a>(
    functions: &'a [(String, rphp::vm::function::UserFunction)],
    function_name: &str,
) -> &'a rphp::vm::quick::QuickLongOpsLoop {
    generic_ops_plan(
        functions,
        function_name,
        |operation| matches!(operation, QuickLongOp::PropertyMethodCall { .. }),
        "compiler should select the generic property-method ops loop",
    )
}

fn generic_property_getter_ops_plan<'a>(
    functions: &'a [(String, rphp::vm::function::UserFunction)],
    function_name: &str,
) -> &'a rphp::vm::quick::QuickLongOpsLoop {
    generic_ops_plan(
        functions,
        function_name,
        |operation| matches!(operation, QuickLongOp::PropertyGetterCall { .. }),
        "compiler should select the generic property-getter ops loop",
    )
}

fn generic_composed_property_ops_plan<'a>(
    functions: &'a [(String, rphp::vm::function::UserFunction)],
    function_name: &str,
) -> &'a rphp::vm::quick::QuickLongOpsLoop {
    generic_ops_plan(
        functions,
        function_name,
        |operation| matches!(operation, QuickLongOp::ComposedPropertyCall { .. }),
        "compiler should select the generic composed-property ops loop",
    )
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

#[test]
fn generic_json_projection_and_property_mutator_enter_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericJsonPropertyJitBox<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function add(T $value): void { $this->total = $this->total + $value; }
}
function genericJsonPropertyTotal($box, $json) {
    $checksum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $row = json_decode($json, true);
        $box->add($row['value']);
        $checksum += $i;
    }
    return $i . ':' . $box->total . ':' . $checksum . ':' . $row['value'];
}
echo genericJsonPropertyTotal(
    new GenericJsonPropertyJitBox::<int>(0),
    '{"value":7}'
);
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:700000:4999950000:7");

    let plan = generic_property_ops_plan(&functions, "genericJsonPropertyTotal");
    assert!(
        plan.ops
            .iter()
            .any(|operation| matches!(operation, QuickLongOp::JsonProjectionStep { .. }))
    );
    assert!(
        plan.native_jit().is_straight_compiled(),
        "ops: {:?}",
        plan.ops
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn nested_json_projection_arguments_enter_one_generic_property_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericNestedJsonPropertyJitBox<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function addPair(T $left, T $right): void {
        $this->total = $this->total + $left;
        $this->total = $this->total + $right;
    }
}
function genericNestedJsonPropertyTotal($box, $json) {
    $checksum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $row = json_decode($json, true);
        $box->addPair($row['left'], $row['nested']['right']);
        $checksum += $i;
    }
    return $i . ':' . $box->total . ':' . $checksum;
}
echo genericNestedJsonPropertyTotal(
    new GenericNestedJsonPropertyJitBox::<int>(0),
    '{"left":3,"nested":{"right":4}}'
);
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:700000:4999950000");

    let plan = generic_property_ops_plan(&functions, "genericNestedJsonPropertyTotal");
    assert_eq!(
        plan.typed_invariant_source
            .as_ref()
            .expect("JSON projections should be retained")
            .projections
            .len(),
        2
    );
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn derived_json_string_length_argument_enters_one_generic_property_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericDerivedJsonPropertyJitBox<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function addPair(T $left, T $right): void {
        $this->total = $this->total + $left;
        $this->total = $this->total + $right;
    }
}
function genericDerivedJsonPropertyTotal($box, $json) {
    $checksum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $row = json_decode($json, true);
        $box->addPair($row['value'], strlen($row['nested']['name']));
        $checksum += $i;
    }
    return $i . ':' . $box->total . ':' . $checksum . ':' . $row['nested']['name'];
}
echo genericDerivedJsonPropertyTotal(
    new GenericDerivedJsonPropertyJitBox::<int>(0),
    '{"value":3,"nested":{"name":"rphp!"}}'
);
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:800000:4999950000:rphp!");

    let plan = generic_property_ops_plan(&functions, "genericDerivedJsonPropertyTotal");
    let source = plan
        .typed_invariant_source
        .as_ref()
        .expect("derived JSON projections should be retained");
    assert_eq!(source.projections.len(), 3);
    assert_eq!(source.long_output_mask.count_ones(), 2);
    assert_eq!(source.string_output_mask.count_ones(), 1);
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn non_string_derived_json_argument_replays_canonically() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericInvalidDerivedJsonPropertyJitBox<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function add(T $value): void { $this->total = $this->total + $value; }
}
function genericInvalidDerivedJsonPropertyTotal($box, $json) {
    for ($i = 0; $i < 100000; $i++) {
        $row = json_decode($json, true);
        $box->add(strlen($row['name']));
    }
    return $box->total;
}
echo genericInvalidDerivedJsonPropertyTotal(
    new GenericInvalidDerivedJsonPropertyJitBox::<int>(0),
    '{"name":"abc"}'
) . '|';
echo genericInvalidDerivedJsonPropertyTotal(
    new GenericInvalidDerivedJsonPropertyJitBox::<int>(0),
    '{"name":123}'
);
"#,
    );
    result.unwrap();
    assert_eq!(output, "300000|300000");

    let plan = generic_property_ops_plan(&functions, "genericInvalidDerivedJsonPropertyTotal");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn invalid_generic_json_projection_replays_before_property_mutation() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericInvalidJsonPropertyJitBox<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function add($value): void { $this->total = $this->total + $value; }
}
function genericInvalidJsonPropertyTotal($box, $json) {
    for ($i = 0; $i < 100000; $i++) {
        $row = json_decode($json, true);
        $box->add($row['value']);
    }
    return $box->total;
}
echo genericInvalidJsonPropertyTotal(
    new GenericInvalidJsonPropertyJitBox::<int>(0),
    '{"value":1}'
) . '|';
$invalid = new GenericInvalidJsonPropertyJitBox::<int>(0);
genericInvalidJsonPropertyTotal($invalid, '{"value":"bad"}');
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "100000|");
    assert!(format!("{error:?}").contains("Unsupported operand types for +"));

    let plan = generic_property_ops_plan(&functions, "genericInvalidJsonPropertyTotal");
    assert!(
        plan.ops
            .iter()
            .any(|operation| matches!(operation, QuickLongOp::JsonProjectionStep { .. }))
    );
    assert!(
        plan.native_jit().is_straight_compiled(),
        "ops: {:?}",
        plan.ops
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn bound_generic_property_getter_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class BoundGenericPropertyGetterJitBox<T : int> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function current(): T { return $this->value; }
}
function genericPropertyReadTotal($box) {
    $sum = 0;
    $checksum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->current();
        $checksum += $i;
    }
    return $i . ':' . $sum . ':' . $checksum;
}
echo genericPropertyReadTotal(new BoundGenericPropertyGetterJitBox::<int>(7));
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:700000:4999950000");

    let plan = generic_property_getter_ops_plan(&functions, "genericPropertyReadTotal");
    assert!(
        plan.native_jit().is_straight_compiled(),
        "ops: {:?}",
        plan.ops
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn generic_property_getter_and_mutator_share_one_native_shadow() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericPropertyPipelineJitBox<T : int> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function current(): T { return $this->value; }
    public function add(T $value): void { $this->value = $this->value + $value; }
}
function genericPropertyPipeline($box) {
    $sum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->current();
        $box->add(1);
    }
    return $i . ':' . $sum . ':' . $box->value;
}
echo genericPropertyPipeline(new GenericPropertyPipelineJitBox::<int>(1));
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:5000050000:100001");

    let plan = generic_property_getter_ops_plan(&functions, "genericPropertyPipeline");
    assert!(
        plan.ops
            .iter()
            .any(|operation| matches!(operation, QuickLongOp::PropertyMethodCall { .. }))
    );
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn generic_composed_property_call_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericComposedPropertySource<T : int> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function current(): T { return $this->value; }
}
class GenericComposedPropertyTarget<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function add(T $value): void { $this->total = $this->total + $value; }
}
function genericComposedPropertyTotal($target, $source) {
    $checksum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $target->add($source->current());
        $checksum += $i;
    }
    return $i . ':' . $target->total . ':' . $checksum;
}
echo genericComposedPropertyTotal(
    new GenericComposedPropertyTarget::<int>(0),
    new GenericComposedPropertySource::<int>(7)
);
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:700000:4999950000");

    let plan = generic_composed_property_ops_plan(&functions, "genericComposedPropertyTotal");
    assert!(
        plan.native_jit().is_straight_compiled(),
        "ops: {:?}",
        plan.ops
    );
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn generic_composed_property_with_conditional_update_enters_one_native_region() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericConditionalPropertySource<T : int> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function current(): T { return $this->value; }
}
class GenericConditionalPropertyTarget<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function add(T $value): void { $this->total = $this->total + $value; }
}
function genericConditionalPropertyTotal($target, $source) {
    $selected = 0;
    for ($i = 0; $i < 100000; $i++) {
        $target->add($source->current());
        if (($i % 2) == 0) {
            $selected += $i;
        }
    }
    return $i . ':' . $target->total . ':' . $selected;
}
echo genericConditionalPropertyTotal(
    new GenericConditionalPropertyTarget::<int>(0),
    new GenericConditionalPropertySource::<int>(7)
);
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:700000:2499950000");

    let plan = generic_composed_property_ops_plan(&functions, "genericConditionalPropertyTotal");
    assert!(
        plan.ops
            .iter()
            .any(|operation| matches!(operation, QuickLongOp::ConditionalAddAssign { .. }))
    );
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn generic_conditional_overflow_keeps_prior_composed_property_call() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericConditionalOverflowSource<T : int> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function current(): T { return $this->value; }
}
class GenericConditionalOverflowTarget<T : int> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function add(T $value): void { $this->total = $this->total + $value; }
}
function genericConditionalOverflow($target, $source) {
    $selected = 9223372036854725808;
    $one = 1;
    for ($i = 0; $i < 100000; $i++) {
        $target->add($source->current());
        if (($i % 2) == 0) {
            $selected += $one;
        }
    }
    return $target->total;
}
echo genericConditionalOverflow(
    new GenericConditionalOverflowTarget::<int>(0),
    new GenericConditionalOverflowSource::<int>(1)
);
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000");

    let plan = generic_composed_property_ops_plan(&functions, "genericConditionalOverflow");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[test]
fn generic_composed_property_call_captures_same_object_getter_before_mutation() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericComposedAliasingBox<T : int> {
    public T $value;
    public T $total;
    public function __construct(T $value, T $total) {
        $this->value = $value;
        $this->total = $total;
    }
    public function current(): T { return $this->value; }
    public function advance(T $original): void {
        $this->value = $this->value + 1;
        $this->total = $this->total + $original;
    }
}
function genericComposedAliasingTotal($box) {
    for ($i = 0; $i < 100000; $i++) {
        $box->advance($box->current());
    }
    return $i . ':' . $box->value . ':' . $box->total;
}
echo genericComposedAliasingTotal(new GenericComposedAliasingBox::<int>(1, 0));
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000:100001:5000050000");

    let plan = generic_composed_property_ops_plan(&functions, "genericComposedAliasingTotal");
    assert!(plan.native_jit().is_straight_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[test]
fn generic_composed_property_overflow_replays_one_transactional_call() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericComposedOverflowSource<T : int> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function current(): T { return $this->value; }
}
class GenericComposedOverflowTarget<T : int> {
    public $large;
    public $calls;
    public function __construct($large) {
        $this->large = $large;
        $this->calls = 0;
    }
    public function add(T $value): void {
        $this->calls = $this->calls + 1;
        $this->large = $this->large + $value;
    }
}
function genericComposedOverflow($target, $source) {
    for ($i = 0; $i < 100000; $i++) {
        $target->add($source->current());
    }
    return $target->calls;
}
echo genericComposedOverflow(
    new GenericComposedOverflowTarget::<int>(9223372036854675808),
    new GenericComposedOverflowSource::<int>(1)
);
"#,
    );
    result.unwrap();
    assert_eq!(output, "100000");

    let plan = generic_composed_property_ops_plan(&functions, "genericComposedOverflow");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert!(plan.native_jit().native_chunks() > 1);
    assert_eq!(plan.native_jit().side_exits(), 1);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_composed_property_mismatch_replays_canonical_outer_boundary() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class ReifiedComposedPropertySource<T> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function current(): T { return $this->value; }
}
class ReifiedComposedPropertyTarget<T> {
    public T $total;
    public function __construct(T $total) { $this->total = $total; }
    public function add(T $value): void { $this->total = $this->total + $value; }
}
function reifiedComposedPropertyTotal($target, $source) {
    for ($i = 0; $i < 100000; $i++) {
        $target->add($source->current());
    }
    return $target->total;
}
echo reifiedComposedPropertyTotal(
    new ReifiedComposedPropertyTarget::<int>(0),
    new ReifiedComposedPropertySource::<int>(1)
) . '|';
reifiedComposedPropertyTotal(
    new ReifiedComposedPropertyTarget::<int>(0),
    new ReifiedComposedPropertySource::<string>('wrong')
);
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "100000|");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to ReifiedComposedPropertyTarget::add()"),
        "{rendered}"
    );
    assert!(rendered.contains("reified class type"), "{rendered}");

    let plan = generic_composed_property_ops_plan(&functions, "reifiedComposedPropertyTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
    assert_eq!(plan.native_jit().side_exits(), 0);
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_property_getter_mismatch_replays_canonical_caller_operation() {
    let (_, functions, result, output) = compile_and_execute(
        r#"<?php
class GenericPropertyGetterJitBox<T> {
    public T $value;
    public function __construct(T $value) { $this->value = $value; }
    public function current(): T { return $this->value; }
}
function genericPropertyReadTotal($box) {
    $sum = 0;
    $checksum = 0;
    for ($i = 0; $i < 100000; $i++) {
        $sum += $box->current();
        $checksum += $i;
    }
    return $sum . ':' . $checksum;
}
echo genericPropertyReadTotal(new GenericPropertyGetterJitBox::<int>(7)) . '|';
genericPropertyReadTotal(new GenericPropertyGetterJitBox::<string>('seed'));
"#,
    );
    let error = result.unwrap_err();
    assert_eq!(output, "700000:4999950000|");
    let rendered = format!("{error:?}");
    assert!(rendered.contains("Unsupported operand types"), "{rendered}");

    let plan = generic_property_getter_ops_plan(&functions, "genericPropertyReadTotal");
    assert_eq!(plan.native_jit().native_entries(), 1);
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
fn generic_void_property_mutator_with_value_return_is_rejected_during_compilation() {
    let source = r#"<?php
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
"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let error = match Compiler::new().compile(&statements) {
        Ok(_) => panic!("invalid void return unexpectedly compiled"),
        Err(error) => error,
    };
    assert!(
        error.contains("A void method must not return a value"),
        "{error}"
    );
}
