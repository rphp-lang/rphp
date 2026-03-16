/// Tests for try/catch/finally
mod common;
use common::{run_php, run_php_expect_error};

#[test]
fn test_try_catch_basic() {
    assert_eq!(run_php(r#"<?php
try {
    throw "error!";
} catch (Exception $e) {
    echo "caught: " . $e;
}
"#), "caught: error!");
}

#[test]
fn test_try_catch_no_throw() {
    assert_eq!(run_php(r#"<?php
try {
    echo "ok";
} catch (Exception $e) {
    echo "caught";
}
"#), "ok");
}

#[test]
fn test_try_catch_code_after() {
    assert_eq!(run_php(r#"<?php
try {
    throw "err";
} catch (Exception $e) {
    echo "caught";
}
echo " done";
"#), "caught done");
}

#[test]
fn test_try_catch_throw_skips_rest() {
    assert_eq!(run_php(r#"<?php
try {
    echo "before";
    throw "err";
    echo "after";
} catch (Exception $e) {
    echo " caught";
}
"#), "before caught");
}

#[test]
fn test_try_catch_exception_value() {
    assert_eq!(run_php(r#"<?php
try {
    throw "specific error";
} catch (Exception $e) {
    echo $e;
}
"#), "specific error");
}

#[test]
fn test_try_catch_integer_exception() {
    assert_eq!(run_php(r#"<?php
try {
    throw 42;
} catch (Exception $e) {
    echo $e;
}
"#), "42");
}

#[test]
fn test_uncaught_exception_is_fatal() {
    let err = run_php_expect_error("<?php throw \"boom\";");
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Uncaught"), "Expected uncaught exception, got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_try_catch_in_function() {
    assert_eq!(run_php(r#"<?php
function risky() {
    throw "oops";
}
try {
    risky();
} catch (Exception $e) {
    echo "caught: " . $e;
}
"#), "caught: oops");
}

#[test]
fn test_nested_try_catch() {
    assert_eq!(run_php(r#"<?php
try {
    try {
        throw "inner";
    } catch (Exception $e) {
        echo "inner: " . $e;
    }
    echo " outer ok";
} catch (Exception $e2) {
    echo "outer: " . $e2;
}
"#), "inner: inner outer ok");
}

#[test]
fn test_try_finally_normal_flow() {
    // Finally runs even when no exception
    assert_eq!(run_php(r#"<?php
try {
    echo "try";
} finally {
    echo " finally";
}
"#), "try finally");
}

#[test]
fn test_try_catch_finally_catch_flow() {
    // Finally runs after catch
    assert_eq!(run_php(r#"<?php
try {
    throw "err";
} catch (Exception $e) {
    echo "caught";
} finally {
    echo " finally";
}
"#), "caught finally");
}

#[test]
fn test_try_finally_on_throw() {
    // Finally runs even when throw occurs and there's no catch
    let err = run_php_expect_error(r#"<?php
try {
    echo "before ";
    throw "boom";
} finally {
    echo "finally";
}
"#);
    // Finally should have run (producing output) but the exception is still uncaught
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Uncaught"), "Expected uncaught exception, got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_try_catch_finally_no_throw() {
    // Normal flow: try + finally, no exception
    assert_eq!(run_php(r#"<?php
try {
    echo "ok";
} catch (Exception $e) {
    echo "caught";
} finally {
    echo " done";
}
"#), "ok done");
}

#[test]
fn test_return_inside_try_runs_finally() {
    // PHP: finally runs even when return is in try block
    assert_eq!(run_php(r#"<?php
function f() {
    try {
        return "T";
    } finally {
        echo "F";
    }
}
echo f();
"#), "FT");
}

#[test]
fn test_return_inside_catch_runs_finally() {
    assert_eq!(run_php(r#"<?php
function f() {
    try {
        throw "err";
    } catch (Exception $e) {
        return "C";
    } finally {
        echo "F";
    }
}
echo f();
"#), "FC");
}

#[test]
fn test_nested_try_finally_exception_propagates() {
    // Inner finally runs, then outer catch handles the exception
    assert_eq!(run_php(r#"<?php
try {
    try {
        throw "boom";
    } finally {
        echo "F ";
    }
} catch (Exception $e) {
    echo "caught: " . $e;
}
"#), "F caught: boom");
}

#[test]
fn test_throw_undef_in_try_finally_is_uncaught() {
    // Regression: Value::undef() sentinel must not collide with real undef throw.
    // `throw $x` where $x is undefined inside try/finally must still be an uncaught fatal,
    // not silently swallowed as a deferred return.
    let err = run_php_expect_error(r#"<?php
function f() {
    try {
        throw $x;
    } finally {
        echo "F";
    }
}
f();
"#);
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Uncaught"), "Expected uncaught exception, got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_nested_finally_return_not_lost() {
    // Regression: pending_return_after_finally must be per-frame, not global.
    // Inner function returning through its own try/finally must not clobber
    // outer function's pending return state.
    assert_eq!(run_php(r#"<?php
function inner() {
    try {
        return "i";
    } finally {
    }
}
function outer() {
    try {
        return inner() . "o";
    } finally {
        echo "F";
    }
}
echo outer();
"#), "Fio");
}

#[test]
fn test_return_in_finally_suppresses_exception() {
    // PHP semantics: return inside finally suppresses any pending exception.
    // After f() returns, no exception should leak into the caller.
    assert_eq!(run_php(r#"<?php
function f() {
    try {
        throw "err";
    } finally {
        return 1;
    }
}
function g() {
    try {
        return "Z";
    } finally {
    }
}
echo "F" . f() . "G" . g();
"#), "F1GZ");
}
