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
