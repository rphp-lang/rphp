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
    // Exception accepts message, code and previous; a fourth public argument
    // must still fail the internal-function arity check.
    let err = run_php_expect_error(
        r#"<?php
try {
    throw new Exception("msg", 1, null, "extra");
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

#[test]
fn throwable_origin_is_the_creation_site_and_survives_a_later_throw() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\n$stored = new Exception('made');\ntry {\n    throw $stored;\n} catch (Throwable $caught) {\n    echo $caught->getFile(), ':', $caught->getLine();\n}",
            "/fixture/throwable-origin.php",
            "/fixture",
        ),
        "/fixture/throwable-origin.php:2"
    );
}

#[test]
fn throwable_origin_exists_before_a_user_constructor_runs() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nclass WrappedOrigin extends Exception {\n    public function __construct() {\n        echo $this->getFile(), ':', $this->getLine();\n        parent::__construct('wrapped');\n    }\n}\nnew WrappedOrigin();",
            "/fixture/wrapped-origin.php",
            "/fixture",
        ),
        "/fixture/wrapped-origin.php:8"
    );
}

#[test]
fn root_created_exception_keeps_its_creation_trace_when_thrown_from_a_function() {
    let error = run_php_expect_error_with_source_context(
        "<?php\n$stored = new Exception('made');\nfunction release_it($value) {\n    throw $value;\n}\nrelease_it($stored);",
        "/fixture/root-created.php",
        "/fixture",
    );

    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message)
            if message == "Uncaught Exception: made in /fixture/root-created.php:2\nStack trace:\n#0 {main}\n  thrown in /fixture/root-created.php on line 2"
    ));
}

#[test]
fn root_uncaught_throwable_rendering_omits_the_colon_for_an_empty_message() {
    let error = run_php_expect_error_with_source_context(
        "<?php\nthrow new Exception();",
        "/fixture/empty-exception.php",
        "/fixture",
    );

    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message)
            if message == "Uncaught Exception in /fixture/empty-exception.php:2\nStack trace:\n#0 {main}\n  thrown in /fixture/empty-exception.php on line 2"
    ));
}

#[test]
fn runtime_constant_array_unpack_error_uses_the_spread_location() {
    let error = run_php_expect_error_with_source_context(
        "<?php\nconst NON_ARRAY_SOURCE = 17;\nconst ITEMS = [\n    ...NON_ARRAY_SOURCE,\n];",
        "/fixture/unpack-origin.php",
        "/fixture",
    );

    let rphp::vm::execute::VmError::Fatal(message) = error else {
        panic!("expected a fatal error");
    };
    assert_eq!(
        message,
        "Uncaught Error: Only arrays can be unpacked in constant expression in /fixture/unpack-origin.php:4\nStack trace:\n#0 {main}\n  thrown in /fixture/unpack-origin.php on line 4"
    );
}
