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
