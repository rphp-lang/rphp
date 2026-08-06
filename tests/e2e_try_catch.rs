/// Tests for try/catch/finally
mod common;
use common::{run_php, run_php_expect_error};

#[test]
fn test_try_catch_basic() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("error!");
} catch (Exception $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: error!"
    );
}

#[test]
fn test_try_catch_no_throw() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "ok";
} catch (Exception $e) {
    echo "caught";
}
"#
        ),
        "ok"
    );
}

#[test]
fn test_try_catch_code_after() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("err");
} catch (Exception $e) {
    echo "caught";
}
echo " done";
"#
        ),
        "caught done"
    );
}

#[test]
fn test_try_catch_throw_skips_rest() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "before";
    throw new Exception("err");
    echo "after";
} catch (Exception $e) {
    echo " caught";
}
"#
        ),
        "before caught"
    );
}

#[test]
fn test_try_catch_exception_value() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("specific error");
} catch (Exception $e) {
    echo $e->getMessage();
}
"#
        ),
        "specific error"
    );
}

#[test]
fn test_try_catch_exception_with_getmessage() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("hello world");
} catch (Exception $e) {
    echo $e->getMessage();
}
"#
        ),
        "hello world"
    );
}

#[test]
fn test_uncaught_exception_is_fatal() {
    let err = run_php_expect_error(r#"<?php throw new Exception("boom");"#);
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught"),
                "Expected uncaught exception, got: {}",
                msg
            );
            assert!(
                msg.contains("boom"),
                "Expected message in error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_try_catch_in_function() {
    assert_eq!(
        run_php(
            r#"<?php
function risky() {
    throw new Exception("oops");
}
try {
    risky();
} catch (Exception $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: oops"
    );
}

#[test]
fn test_nested_try_catch() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    try {
        throw new Exception("inner");
    } catch (Exception $e) {
        echo "inner: " . $e->getMessage();
    }
    echo " outer ok";
} catch (Exception $e2) {
    echo "outer: " . $e2->getMessage();
}
"#
        ),
        "inner: inner outer ok"
    );
}

#[test]
fn test_try_finally_normal_flow() {
    // Finally runs even when no exception
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "try";
} finally {
    echo " finally";
}
"#
        ),
        "try finally"
    );
}

#[test]
fn test_try_catch_finally_catch_flow() {
    // Finally runs after catch
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("err");
} catch (Exception $e) {
    echo "caught";
} finally {
    echo " finally";
}
"#
        ),
        "caught finally"
    );
}

#[test]
fn test_try_finally_on_throw() {
    // Finally runs even when throw occurs and there's no catch
    let err = run_php_expect_error(
        r#"<?php
try {
    echo "before ";
    throw new Exception("boom");
} finally {
    echo "finally";
}
"#,
    );
    // Finally should have run (producing output) but the exception is still uncaught
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught"),
                "Expected uncaught exception, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_try_catch_finally_no_throw() {
    // Normal flow: try + finally, no exception
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "ok";
} catch (Exception $e) {
    echo "caught";
} finally {
    echo " done";
}
"#
        ),
        "ok done"
    );
}

#[test]
fn test_return_inside_try_runs_finally() {
    // PHP: finally runs even when return is in try block
    assert_eq!(
        run_php(
            r#"<?php
function f() {
    try {
        return "T";
    } finally {
        echo "F";
    }
}
echo f();
"#
        ),
        "FT"
    );
}

#[test]
fn test_return_inside_catch_runs_finally() {
    assert_eq!(
        run_php(
            r#"<?php
function f() {
    try {
        throw new Exception("err");
    } catch (Exception $e) {
        return "C";
    } finally {
        echo "F";
    }
}
echo f();
"#
        ),
        "FC"
    );
}

#[test]
fn test_nested_try_finally_exception_propagates() {
    // Inner finally runs, then outer catch handles the exception
    assert_eq!(
        run_php(
            r#"<?php
try {
    try {
        throw new Exception("boom");
    } finally {
        echo "F ";
    }
} catch (Exception $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "F caught: boom"
    );
}

#[test]
fn test_throw_undef_in_try_finally_is_fatal() {
    // `throw $x` where $x is undefined → fatal: not a Throwable
    let err = run_php_expect_error(
        r#"<?php
function f() {
    try {
        throw $x;
    } finally {
        echo "F";
    }
}
f();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Throwable"),
                "Expected Throwable error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_nested_finally_return_not_lost() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "Fio"
    );
}

#[test]
fn test_return_in_finally_suppresses_exception() {
    // PHP semantics: return inside finally suppresses any pending exception.
    assert_eq!(
        run_php(
            r#"<?php
function f() {
    try {
        throw new Exception("err");
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
"#
        ),
        "F1GZ"
    );
}

// ============================================================
// Error vs Exception hierarchy tests
// ============================================================

#[test]
fn test_catch_error_catches_undefined_function() {
    // PHP 8: undefined function throws Error, catchable by catch(Error)
    assert_eq!(
        run_php(
            r#"<?php
try {
    nonexistent();
} catch (Error $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: Call to undefined function nonexistent()"
    );
}

#[test]
fn test_catch_exception_does_not_catch_error() {
    // PHP 8: catch(Exception) does NOT catch Error
    let err = run_php_expect_error(
        r#"<?php
try {
    nonexistent();
} catch (Exception $e) {
    echo "caught";
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught Error"),
                "Expected Uncaught Error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_catch_throwable_catches_both() {
    // catch(Throwable) catches both Error and Exception
    assert_eq!(
        run_php(
            r#"<?php
try {
    nonexistent();
} catch (Throwable $e) {
    echo "caught error";
}
try {
    throw new Exception("ex");
} catch (Throwable $e) {
    echo " caught exception";
}
"#
        ),
        "caught error caught exception"
    );
}

#[test]
fn test_catch_error_does_not_catch_exception() {
    // catch(Error) does NOT catch Exception
    let err = run_php_expect_error(
        r#"<?php
try {
    throw new Exception("test");
} catch (Error $e) {
    echo "caught";
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught Exception"),
                "Expected Uncaught Exception, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_catch_typeerror() {
    // TypeError extends Error, catchable by catch(TypeError) and catch(Error)
    assert_eq!(
        run_php(
            r#"<?php
try {
    array_map("nonexistent", [1]);
} catch (TypeError $e) {
    echo "TypeError";
}
"#
        ),
        "TypeError"
    );
}

#[test]
fn test_catch_error_catches_typeerror() {
    // TypeError extends Error, so catch(Error) catches it too
    assert_eq!(
        run_php(
            r#"<?php
try {
    array_map("nonexistent", [1]);
} catch (Error $e) {
    echo "Error";
}
"#
        ),
        "Error"
    );
}

// ============================================================
// throw validation — only Throwable objects allowed
// ============================================================

#[test]
fn test_throw_string_is_fatal() {
    // PHP 8: throw "string" is not allowed
    let err = run_php_expect_error(r#"<?php throw "boom";"#);
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Throwable"),
                "Expected Throwable error, got: {}",
                msg
            );
            assert!(
                msg.contains("string"),
                "Expected 'string' type mentioned, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_throw_integer_is_fatal() {
    let err = run_php_expect_error(r#"<?php throw 42;"#);
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Throwable"),
                "Expected Throwable error, got: {}",
                msg
            );
            assert!(
                msg.contains("int"),
                "Expected 'int' type mentioned, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_throw_non_throwable_object_is_fatal() {
    let err = run_php_expect_error(
        r#"<?php
class Foo {}
throw new Foo();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Throwable"),
                "Expected Throwable error, got: {}",
                msg
            );
            assert!(
                msg.contains("Foo"),
                "Expected class name mentioned, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_throw_null_is_fatal() {
    let err = run_php_expect_error(r#"<?php throw null;"#);
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Throwable"),
                "Expected Throwable error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_throw_error_subclass_is_ok() {
    // User-defined class extending Error should be throwable
    assert_eq!(
        run_php(
            r#"<?php
class MyError extends Error {}
try {
    throw new MyError("custom");
} catch (Error $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: custom"
    );
}

#[test]
fn test_throw_exception_subclass_is_ok() {
    assert_eq!(
        run_php(
            r#"<?php
class AppException extends Exception {}
try {
    throw new AppException("app error");
} catch (Exception $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: app error"
    );
}

#[test]
fn test_new_exception_extra_args_too_many() {
    // Exception("msg", "extra") — only 1 public arg ($message), so 2 is too many
    let err = run_php_expect_error(
        r#"<?php
try {
    throw new Exception("msg", "extra");
} catch (Exception $e) {
    echo $e->getMessage();
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Too many arguments"),
                "Expected too many args, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_getmessage_extra_args_too_many() {
    // getMessage("unused") — getMessage takes 0 public args, so 1 is too many
    let err = run_php_expect_error(
        r#"<?php
try {
    throw new Exception("hello");
} catch (Exception $e) {
    echo $e->getMessage("unused");
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Too many arguments"),
                "Expected too many args, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_new_exception_one_arg_ok() {
    // Exception("msg") — exactly 1 public arg, should work fine
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("msg");
} catch (Exception $e) {
    echo $e->getMessage();
}
"#
        ),
        "msg"
    );
}

#[test]
fn test_getmessage_no_args_ok() {
    // getMessage() with no extra args — normal case
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("hello");
} catch (Exception $e) {
    echo $e->getMessage();
}
"#
        ),
        "hello"
    );
}
