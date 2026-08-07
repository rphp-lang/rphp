/// Tests for array_map, array_filter and array_reduce callback invocation.
mod common;
use common::{run_php, run_php_expect_error};

// ============================================================
// array_map
// ============================================================

#[test]
fn test_array_map_basic() {
    let out = run_php(
        r#"<?php
function double($x) { return $x * 2; }
$arr = [1, 2, 3];
$result = array_map("double", $arr);
echo $result[0] . "," . $result[1] . "," . $result[2];
"#,
    );
    assert_eq!(out, "2,4,6");
}

#[test]
fn test_array_map_strings() {
    let out = run_php(
        r#"<?php
function upper($s) { return strtoupper($s); }
$arr = ["hello", "world"];
$result = array_map("upper", $arr);
echo $result[0] . " " . $result[1];
"#,
    );
    assert_eq!(out, "HELLO WORLD");
}

#[test]
fn test_array_map_preserves_keys() {
    let out = run_php(
        r#"<?php
function inc($x) { return $x + 1; }
$arr = ["a" => 10, "b" => 20];
$result = array_map("inc", $arr);
echo $result["a"] . "," . $result["b"];
"#,
    );
    assert_eq!(out, "11,21");
}

#[test]
fn test_array_map_supports_general_callable_forms() {
    let out = run_php(
        r#"<?php
class MapCallbacks {
    public function triple($value) { return $value * 3; }
    public static function addTen($value) { return $value + 10; }
    public function __invoke($value) { return $value - 1; }
}
$offset = 4;
$closure = function($value) use ($offset) { return $value + $offset; };
$callbacks = new MapCallbacks();
$closureResult = array_map($closure, [1, 2]);
$methodResult = array_map([$callbacks, "triple"], [2, 3]);
$staticResult = array_map(["MapCallbacks", "addTen"], [4, 5]);
$invokeResult = array_map($callbacks, [7, 8]);
echo $closureResult[0] . "," . $closureResult[1] . ":";
echo $methodResult[0] . "," . $methodResult[1] . ":";
echo $staticResult[0] . "," . $staticResult[1] . ":";
echo $invokeResult[0] . "," . $invokeResult[1];
"#,
    );
    assert_eq!(out, "5,6:6,9:14,15:6,7");
}

#[test]
fn test_array_map_with_stdlib_callback() {
    let out = run_php(
        r#"<?php
$arr = ["hello", "world"];
$result = array_map("strlen", $arr);
echo $result[0] . "," . $result[1];
"#,
    );
    assert_eq!(out, "5,5");
}

#[test]
fn test_array_map_closure() {
    let out = run_php(
        r#"<?php
function square($n) { return $n * $n; }
$nums = [1, 2, 3, 4];
$squares = array_map("square", $nums);
echo array_sum($squares);
"#,
    );
    assert_eq!(out, "30");
}

#[test]
fn test_array_map_empty() {
    let out = run_php(
        r#"<?php
function double($x) { return $x * 2; }
$result = array_map("double", []);
echo count($result);
"#,
    );
    assert_eq!(out, "0");
}

// ============================================================
// array_filter
// ============================================================

#[test]
fn test_array_filter_with_callback() {
    let out = run_php(
        r#"<?php
function is_even($x) { return $x % 2 == 0; }
$arr = [1, 2, 3, 4, 5, 6];
$result = array_filter($arr, "is_even");
echo count($result);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_array_filter_without_callback() {
    // Filter by truthiness — removes 0, "", null, false
    let out = run_php(
        r#"<?php
$arr = [0, 1, "", "hello", null, 42];
$result = array_filter($arr);
echo count($result);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_array_filter_preserves_keys() {
    let out = run_php(
        r#"<?php
function gt_two($x) { return $x > 2; }
$arr = [1, 2, 3, 4];
$result = array_filter($arr, "gt_two");
$keys = array_keys($result);
echo $keys[0] . "," . $keys[1];
"#,
    );
    assert_eq!(out, "2,3");
}

#[test]
fn test_array_filter_supports_closure_and_method_callbacks() {
    let out = run_php(
        r#"<?php
class FilterCallbacks {
    public function keepOdd($value) { return $value & 1; }
}
$minimum = 2;
$closure = function($value) use ($minimum) { return $value > $minimum; };
$callbacks = new FilterCallbacks();
$closureResult = array_filter([1, 2, 3, 4], $closure);
$methodResult = array_filter([1, 2, 3, 4], [$callbacks, "keepOdd"]);
echo count($closureResult) . ":" . $closureResult[2] . "," . $closureResult[3] . ":";
echo count($methodResult) . ":" . $methodResult[0] . "," . $methodResult[2];
"#,
    );
    assert_eq!(out, "2:3,4:2:1,3");
}

#[test]
fn test_array_reduce_scalar_and_general_callbacks() {
    let out = run_php(
        r#"<?php
function sumValues($carry, $value) { return $carry + $value; }
$factor = 2;
$closure = function($carry, $value) use ($factor) { return $carry + $value * $factor; };
echo array_reduce([1, 2, 3, 4], "sumValues", 0) . ":";
echo array_reduce([1, 2, 3, 4], $closure, 0);
"#,
    );
    assert_eq!(out, "10:20");
}

#[test]
fn test_nested_scalar_callback_pipeline_preserves_result() {
    let out = run_php(
        r#"<?php
function pipelineMap($value) { return $value * 3 + 1; }
function pipelineKeep($value) { return $value & 1; }
function pipelineSum($carry, $value) { return $carry + $value; }
$result = array_reduce(
    array_filter(array_map("pipelineMap", [0, 1, 2, 3, 4, 5]), "pipelineKeep"),
    "pipelineSum",
    0
);
echo $result;
"#,
    );
    assert_eq!(out, "21");
}

#[test]
fn test_nested_scalar_callback_pipeline_falls_back_for_double_input() {
    let out = run_php(
        r#"<?php
function fallbackMap($value) { return $value + 1; }
function fallbackKeep($value) { return $value; }
function fallbackSum($carry, $value) { return $carry + $value; }
function runPipeline($values) {
    return array_reduce(
        array_filter(array_map("fallbackMap", $values), "fallbackKeep"),
        "fallbackSum",
        0
    );
}
echo runPipeline([1, 2]) . ":";
$doubleResult = runPipeline([1.5, 2.5]);
echo gettype($doubleResult) . ":" . $doubleResult;
"#,
    );
    assert_eq!(out, "5:double:6");
}

#[test]
fn test_nested_callback_pipeline_keeps_canonical_callback_order() {
    let out = run_php(
        r#"<?php
function orderedMap($value) { echo "m" . $value; return $value + 1; }
function orderedKeep($value) { echo "f" . $value; return $value & 1; }
function orderedSum($carry, $value) { echo "r" . $value; return $carry + $value; }
$result = array_reduce(
    array_filter(array_map("orderedMap", [1, 2, 3]), "orderedKeep"),
    "orderedSum",
    0
);
echo ":" . $result;
"#,
    );
    assert_eq!(out, "m1m2m3f2f3f4r3:3");
}

#[test]
fn test_nested_scalar_callback_pipeline_replays_overflow_canonically() {
    let out = run_php(
        r#"<?php
function overflowMap($value) { return $value + 1; }
function overflowKeep($value) { return 1; }
function overflowSum($carry, $value) { return $carry + $value; }
$result = array_reduce(
    array_filter(array_map("overflowMap", [9223372036854775807]), "overflowKeep"),
    "overflowSum",
    0
);
echo gettype($result);
"#,
    );
    assert_eq!(out, "double");
}

#[test]
fn test_dead_staged_scalar_callback_pipeline_preserves_result() {
    let out = run_php(
        r#"<?php
function stagedMap($value) { return $value * 3 + 1; }
function stagedKeep($value) { return $value & 1; }
function stagedSum($carry, $value) { return $carry + $value; }
function stagedPipeline($values) {
    $mapped = array_map("stagedMap", $values);
    $filtered = array_filter($mapped, "stagedKeep");
    return array_reduce($filtered, "stagedSum", 0);
}
echo stagedPipeline([0, 1, 2, 3, 4, 5]);
"#,
    );
    assert_eq!(out, "21");
}

#[test]
fn test_escaping_staged_callback_pipeline_materializes_results() {
    let out = run_php(
        r#"<?php
function escapingMap($value) { return $value * 3 + 1; }
function escapingKeep($value) { return $value & 1; }
function escapingSum($carry, $value) { return $carry + $value; }
function escapingPipeline($values) {
    $mapped = array_map("escapingMap", $values);
    $filtered = array_filter($mapped, "escapingKeep");
    $sum = array_reduce($filtered, "escapingSum", 0);
    return count($mapped) . ":" . count($filtered) . ":" . $sum;
}
echo escapingPipeline([0, 1, 2, 3, 4, 5]);
"#,
    );
    assert_eq!(out, "6:3:21");
}

#[test]
fn test_initialized_staged_destination_uses_canonical_assignment() {
    let out = run_php(
        r#"<?php
function initializedMap($value) { return $value * 3 + 1; }
function initializedKeep($value) { return $value & 1; }
function initializedSum($carry, $value) { return $carry + $value; }
function initializedPipeline($values, &$mapped) {
    $mapped = array_map("initializedMap", $values);
    $filtered = array_filter($mapped, "initializedKeep");
    return array_reduce($filtered, "initializedSum", 0);
}
$initialized = 99;
$first = initializedPipeline([0, 1, 2, 3, 4, 5], $initialized);
$second = initializedPipeline([0, 1, 2, 3, 4, 5], $undefined);
echo gettype($initialized) . ":" . count($initialized) . ":" . $first . "|";
echo gettype($undefined) . ":" . count($undefined) . ":" . $second;
"#,
    );
    assert_eq!(out, "array:6:21|array:6:21");
}

#[test]
fn test_filter_map_reduce_pipeline_preserves_nested_and_staged_results() {
    let out = run_php(
        r#"<?php
function filterMapKeep($value) { return $value & 1; }
function filterMapMap($value) { return $value * 3 + 1; }
function filterMapSum($carry, $value) { return $carry + $value; }
function nestedFilterMap($values) {
    return array_reduce(
        array_map("filterMapMap", array_filter($values, "filterMapKeep")),
        "filterMapSum",
        0
    );
}
function stagedFilterMap($values) {
    $filtered = array_filter($values, "filterMapKeep");
    $mapped = array_map("filterMapMap", $filtered);
    return array_reduce($mapped, "filterMapSum", 0);
}
echo nestedFilterMap([0, 1, 2, 3, 4, 5]) . ":";
echo stagedFilterMap([0, 1, 2, 3, 4, 5]);
"#,
    );
    assert_eq!(out, "30:30");
}

#[test]
fn test_filter_map_pipeline_keeps_canonical_impure_order() {
    let out = run_php(
        r#"<?php
function orderedFilterMapKeep($value) { echo "f" . $value; return $value & 1; }
function orderedFilterMapMap($value) { echo "m" . $value; return $value * 3 + 1; }
function orderedFilterMapSum($carry, $value) { echo "r" . $value; return $carry + $value; }
$result = array_reduce(
    array_map("orderedFilterMapMap", array_filter([0, 1, 2, 3, 4, 5], "orderedFilterMapKeep")),
    "orderedFilterMapSum",
    0
);
echo ":" . $result;
"#,
    );
    assert_eq!(out, "f0f1f2f3f4f5m1m3m5r4r10r16:30");
}

#[test]
fn test_filter_map_pipeline_replays_double_input_canonically() {
    let out = run_php(
        r#"<?php
function doubleFilterMapKeep($value) { return 1; }
function doubleFilterMapMap($value) { return $value + 1; }
function doubleFilterMapSum($carry, $value) { return $carry + $value; }
function doubleFilterMapPipeline($values) {
    return array_reduce(
        array_map("doubleFilterMapMap", array_filter($values, "doubleFilterMapKeep")),
        "doubleFilterMapSum",
        0
    );
}
echo doubleFilterMapPipeline([1, 2]) . ":";
$double = doubleFilterMapPipeline([1.5, 2.5]);
echo gettype($double) . ":" . $double;
"#,
    );
    assert_eq!(out, "5:double:6");
}

#[test]
fn test_filter_map_staged_escape_and_reference_destination_materialize() {
    let out = run_php(
        r#"<?php
function materializedFilterMapKeep($value) { return $value & 1; }
function materializedFilterMapMap($value) { return $value * 3 + 1; }
function materializedFilterMapSum($carry, $value) { return $carry + $value; }
function escapingFilterMap($values) {
    $filtered = array_filter($values, "materializedFilterMapKeep");
    $mapped = array_map("materializedFilterMapMap", $filtered);
    $sum = array_reduce($mapped, "materializedFilterMapSum", 0);
    return count($filtered) . ":" . count($mapped) . ":" . $sum;
}
function referencedFilterMap($values, &$filtered) {
    $filtered = array_filter($values, "materializedFilterMapKeep");
    $mapped = array_map("materializedFilterMapMap", $filtered);
    return array_reduce($mapped, "materializedFilterMapSum", 0);
}
echo escapingFilterMap([0, 1, 2, 3, 4, 5]) . "|";
$sum = referencedFilterMap([0, 1, 2, 3, 4, 5], $external);
echo gettype($external) . ":" . count($external) . ":" . $sum;
"#,
    );
    assert_eq!(out, "3:3:30|array:3:30");
}

#[test]
fn test_json_callback_pipeline_preserves_all_admitted_shapes() {
    let out = run_php(
        r#"<?php
function jsonPipelineMap($value) { return $value * 3 + 1; }
function jsonPipelineKeep($value) { return $value & 1; }
function jsonPipelineSum($carry, $value) { return $carry + $value; }
function nestedJsonMapFilter($values) {
    return json_encode(array_reduce(
        array_filter(array_map("jsonPipelineMap", $values), "jsonPipelineKeep"),
        "jsonPipelineSum",
        0
    ));
}
function stagedJsonMapFilter($values) {
    $mapped = array_map("jsonPipelineMap", $values);
    $filtered = array_filter($mapped, "jsonPipelineKeep");
    return json_encode(array_reduce($filtered, "jsonPipelineSum", 0));
}
function nestedJsonFilterMap($values) {
    return json_encode(array_reduce(
        array_map("jsonPipelineMap", array_filter($values, "jsonPipelineKeep")),
        "jsonPipelineSum",
        0
    ));
}
function stagedJsonFilterMap($values) {
    $filtered = array_filter($values, "jsonPipelineKeep");
    $mapped = array_map("jsonPipelineMap", $filtered);
    return json_encode(array_reduce($mapped, "jsonPipelineSum", 0));
}
$values = [0, 1, 2, 3, 4, 5];
echo nestedJsonMapFilter($values) . ":";
echo stagedJsonMapFilter($values) . ":";
echo nestedJsonFilterMap($values) . ":";
echo stagedJsonFilterMap($values);
"#,
    );
    assert_eq!(out, "21:21:30:30");
}

#[test]
fn test_json_callback_pipeline_falls_back_for_double_and_impure_callbacks() {
    let out = run_php(
        r#"<?php
function jsonFallbackMap($value) { return $value + 1; }
function jsonFallbackKeep($value) { return 1; }
function jsonFallbackSum($carry, $value) { return $carry + $value; }
function jsonFallbackPipeline($values) {
    return json_encode(array_reduce(
        array_filter(array_map("jsonFallbackMap", $values), "jsonFallbackKeep"),
        "jsonFallbackSum",
        0
    ));
}
echo jsonFallbackPipeline([1, 2]) . ":";
echo jsonFallbackPipeline([1.5, 2.5]) . "|";
function jsonOrderedMap($value) { echo "m" . $value; return $value + 1; }
function jsonOrderedKeep($value) { echo "f" . $value; return $value & 1; }
function jsonOrderedSum($carry, $value) { echo "r" . $value; return $carry + $value; }
$result = json_encode(array_reduce(
    array_filter(array_map("jsonOrderedMap", [1, 2, 3]), "jsonOrderedKeep"),
    "jsonOrderedSum",
    0
));
echo ":" . $result;
"#,
    );
    assert_eq!(out, "5:6.0|m1m2m3f2f3f4r3:3");
}

#[test]
fn test_json_staged_pipeline_materializes_escaping_intermediates() {
    let out = run_php(
        r#"<?php
function jsonEscapingMap($value) { return $value * 3 + 1; }
function jsonEscapingKeep($value) { return $value & 1; }
function jsonEscapingSum($carry, $value) { return $carry + $value; }
function jsonEscapingPipeline($values) {
    $mapped = array_map("jsonEscapingMap", $values);
    $filtered = array_filter($mapped, "jsonEscapingKeep");
    $encoded = json_encode(array_reduce($filtered, "jsonEscapingSum", 0));
    return count($mapped) . ":" . count($filtered) . ":" . $encoded;
}
echo jsonEscapingPipeline([0, 1, 2, 3, 4, 5]);
"#,
    );
    assert_eq!(out, "6:3:21");
}

#[test]
fn test_json_pipeline_respects_namespaced_json_encode_shadow() {
    let out = run_php(
        r#"<?php
namespace PipelineJsonShadow;
function json_encode($value) { return "custom:" . $value; }
function shadowMap($value) { return $value * 3 + 1; }
function shadowKeep($value) { return $value & 1; }
function shadowSum($carry, $value) { return $carry + $value; }
echo json_encode(array_reduce(
    array_filter(
        array_map("PipelineJsonShadow\\shadowMap", [0, 1, 2, 3, 4, 5]),
        "PipelineJsonShadow\\shadowKeep"
    ),
    "PipelineJsonShadow\\shadowSum",
    0
));
"#,
    );
    assert_eq!(out, "custom:21");
}

#[test]
fn test_array_filter_all_pass() {
    let out = run_php(
        r#"<?php
function is_positive($x) { return $x > 0; }
$arr = [1, 2, 3];
$result = array_filter($arr, "is_positive");
echo count($result);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_array_filter_none_pass() {
    let out = run_php(
        r#"<?php
function is_negative($x) { return $x < 0; }
$arr = [1, 2, 3];
$result = array_filter($arr, "is_negative");
echo count($result);
"#,
    );
    assert_eq!(out, "0");
}

#[test]
fn test_array_filter_empty() {
    let out = run_php(
        r#"<?php
function always_true($x) { return true; }
$result = array_filter([], "always_true");
echo count($result);
"#,
    );
    assert_eq!(out, "0");
}

// ============================================================
// Combined usage
// ============================================================

#[test]
fn test_map_then_filter() {
    let out = run_php(
        r#"<?php
function double($x) { return $x * 2; }
function gt_five($x) { return $x > 5; }
$arr = [1, 2, 3, 4, 5];
$doubled = array_map("double", $arr);
$filtered = array_filter($doubled, "gt_five");
echo count($filtered);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_filter_then_map() {
    let out = run_php(
        r#"<?php
function is_even($x) { return $x % 2 == 0; }
function square($x) { return $x * $x; }
$arr = [1, 2, 3, 4, 5, 6];
$evens = array_filter($arr, "is_even");
$squares = array_map("square", array_values($evens));
echo $squares[0] . "," . $squares[1] . "," . $squares[2];
"#,
    );
    assert_eq!(out, "4,16,36");
}

#[test]
fn test_array_map_undefined_callback_error() {
    // TypeError from invalid callback is catchable by catch(TypeError) or catch(Error)
    let err = common::run_php_expect_error(
        r#"<?php
$result = array_map("nonexistent", [1, 2, 3]);
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("nonexistent"),
                "Error should mention function name: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got {:?}", other),
    }
}

#[test]
fn test_array_filter_undefined_callback_error() {
    let err = common::run_php_expect_error(
        r#"<?php
$result = array_filter([1, 2, 3], "nonexistent");
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("nonexistent"),
                "Error should mention function name: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got {:?}", other),
    }
}

#[test]
fn test_array_map_callback_error_propagates() {
    // Undefined function inside callback throws Error, propagates as uncaught
    let err = common::run_php_expect_error(
        r#"<?php
function bad($x) { return undefined_fn($x); }
$result = array_map("bad", [1]);
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("undefined"),
                "Error should mention undefined: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got {:?}", other),
    }
}

#[test]
fn test_array_map_throw_caught_by_try_catch() {
    let out = run_php(
        r#"<?php
function cb($x) {
    if ($x > 2) { throw new Exception("too big"); }
    return $x * 10;
}
try {
    $result = array_map("cb", [1, 2, 3]);
    echo "no catch";
} catch (Exception $e) {
    echo "caught:" . $e->getMessage();
}
"#,
    );
    assert_eq!(out, "caught:too big");
}

#[test]
fn test_array_filter_throw_caught_by_try_catch() {
    let out = run_php(
        r#"<?php
function bad_filter($x) {
    if ($x == 2) { throw new Exception("nope"); }
    return true;
}
try {
    $result = array_filter([1, 2, 3], "bad_filter");
    echo "no catch";
} catch (Exception $e) {
    echo "caught:" . $e->getMessage();
}
"#,
    );
    assert_eq!(out, "caught:nope");
}

/// TypeError from undefined callback is NOT catchable by catch(Exception).
#[test]
fn test_array_map_undefined_callback_not_caught_by_exception() {
    let err = run_php_expect_error(
        r#"<?php
try {
    $r = array_map("nonexistent", [1, 2]);
    echo "no catch";
} catch (Exception $e) {
    echo "caught";
}
"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("nonexistent"),
        "expected function name in fatal: {}",
        msg
    );
}

/// TypeError from undefined callback IS catchable by catch(TypeError) or catch(Error).
#[test]
fn test_array_map_undefined_callback_caught_by_typeerror() {
    let out = run_php(
        r#"<?php
try {
    $r = array_map("nonexistent", [1, 2]);
    echo "no catch";
} catch (TypeError $e) {
    echo "caught:" . $e->getMessage();
}
"#,
    );
    assert!(out.contains("caught:"), "Expected catch to fire: {}", out);
    assert!(
        out.contains("nonexistent"),
        "Expected function name: {}",
        out
    );
}

/// Error from undefined function inside callback is NOT catchable by catch(Exception).
#[test]
fn test_array_map_callback_error_not_caught_by_exception() {
    let err = run_php_expect_error(
        r#"<?php
function cb($x) { return nope(); }
try {
    $r = array_map("cb", [1]);
    echo "no catch";
} catch (Exception $e) {
    echo "caught";
}
"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("nope"),
        "expected function name in fatal: {}",
        msg
    );
}

/// Error from undefined function inside callback IS catchable by catch(Error).
#[test]
fn test_array_map_callback_error_caught_by_error() {
    let out = run_php(
        r#"<?php
function cb($x) { return nope(); }
try {
    $r = array_map("cb", [1]);
    echo "no catch";
} catch (Error $e) {
    echo "caught:" . $e->getMessage();
}
"#,
    );
    assert!(out.contains("caught:"), "Expected catch to fire: {}", out);
    assert!(out.contains("nope"), "Expected function name: {}", out);
}

#[test]
fn test_array_map_with_recursive_callback() {
    let out = run_php(
        r#"<?php
function fib($n) {
    if ($n <= 1) { return $n; }
    return fib($n - 1) + fib($n - 2);
}
$nums = [0, 1, 2, 3, 4, 5, 6, 7];
$fibs = array_map("fib", $nums);
echo $fibs[7];
"#,
    );
    assert_eq!(out, "13");
}

// ============================================================
// Error vs Exception hierarchy — regression tests
// ============================================================

/// TypeError from undefined callback in array_filter NOT catchable by catch(Exception).
#[test]
fn test_array_filter_undefined_callback_not_caught_by_exception() {
    let err = run_php_expect_error(
        r#"<?php
try {
    $r = array_filter([1, 2, 3], "nonexistent");
    echo "no catch";
} catch (Exception $e) {
    echo "caught";
}
"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("nonexistent"),
        "expected function name in fatal: {}",
        msg
    );
}

/// Error from undefined function inside filter callback NOT catchable by catch(Exception).
#[test]
fn test_array_filter_callback_error_not_caught_by_exception() {
    let err = run_php_expect_error(
        r#"<?php
function cb_filter($x) { return nope(); }
try {
    $r = array_filter([1], "cb_filter");
    echo "no catch";
} catch (Exception $e) {
    echo "caught";
}
"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("nope"),
        "expected function name in fatal: {}",
        msg
    );
}

/// Verify that elements before the throw ARE processed (partial processing).
#[test]
fn test_array_map_throw_partial_processing() {
    let out = run_php(
        r#"<?php
function cb($x) { if ($x > 2) { throw new Exception("stop"); } echo $x . ","; return $x; }
try { array_map("cb", [1, 2, 3, 4]); } catch (Exception $e) { echo "caught:" . $e->getMessage(); }
"#,
    );
    assert_eq!(out, "1,2,caught:stop");
}

/// Verify that elements before the throw ARE processed in array_filter (partial processing).
#[test]
fn test_array_filter_throw_partial_processing() {
    let out = run_php(
        r#"<?php
function cb_f($x) { if ($x > 2) { throw new Exception("stop"); } echo $x . ","; return true; }
try { array_filter([1, 2, 3, 4], "cb_f"); } catch (Exception $e) { echo "caught:" . $e->getMessage(); }
"#,
    );
    assert_eq!(out, "1,2,caught:stop");
}

/// Throw from inner array_map propagates through outer array_map.
#[test]
fn test_nested_array_map_throw() {
    let out = run_php(
        r#"<?php
function outer($x) { return array_map("inner", $x); }
function inner($x) { if ($x > 2) { throw new Exception("boom"); } return $x * 10; }
try { $r = array_map("outer", [[1,2],[3,4]]); echo "no"; } catch (Exception $e) { echo "caught:" . $e->getMessage(); }
"#,
    );
    assert_eq!(out, "caught:boom");
}

/// Code after try/catch block continues executing after a caught throw.
#[test]
fn test_array_map_throw_then_normal_code_runs() {
    let out = run_php(
        r#"<?php
function cb_throw($x) { throw new Exception("err"); }
try { array_map("cb_throw", [1]); } catch (Exception $e) { echo "caught"; }
echo ":after";
"#,
    );
    assert_eq!(out, "caught:after");
}
