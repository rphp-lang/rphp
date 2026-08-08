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
