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
