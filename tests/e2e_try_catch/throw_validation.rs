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
fn throwable_trace_snapshots_method_calls_and_arguments_at_creation() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nclass TraceProbe {\n    public function outer($value) {\n        return $this->inner($value);\n    }\n    public function inner($value) {\n        return new Exception('stored');\n    }\n}\n$stored = (new TraceProbe())->outer('abcdefghijklmnopqrst');\nforeach ($stored->getTrace() as $index => $frame) {\n    echo $index, ':', $frame['file'], ':', $frame['line'], ':', $frame['class'], $frame['type'], $frame['function'], ':', $frame['args'][0], \"\\n\";\n}\necho $stored->getTraceAsString(), \"\\n\";\ntry { throw $stored; } catch (Throwable $caught) {\n    echo $caught->getLine(), \"\\n\", $caught->getTraceAsString();\n}",
            "/fixture/trace-snapshot.php",
            "/fixture",
        ),
        "0:/fixture/trace-snapshot.php:4:TraceProbe->inner:abcdefghijklmnopqrst\n1:/fixture/trace-snapshot.php:10:TraceProbe->outer:abcdefghijklmnopqrst\n#0 /fixture/trace-snapshot.php(4): TraceProbe->inner('abcdefghijklmno...')\n#1 /fixture/trace-snapshot.php(10): TraceProbe->outer('abcdefghijklmno...')\n#2 {main}\n7\n#0 /fixture/trace-snapshot.php(4): TraceProbe->inner('abcdefghijklmno...')\n#1 /fixture/trace-snapshot.php(10): TraceProbe->outer('abcdefghijklmno...')\n#2 {main}"
    );
}

#[test]
fn nested_uncaught_trace_uses_each_callers_source_line() {
    let error = run_php_expect_error_with_source_context(
        "<?php\nfunction outer($value) {\n    inner($value);\n}\nfunction inner($value) {\n    throw new Exception('boom');\n}\nouter(42);",
        "/fixture/nested-trace.php",
        "/fixture",
    );

    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message)
            if message == "Uncaught Exception: boom in /fixture/nested-trace.php:6\nStack trace:\n#0 /fixture/nested-trace.php(3): inner(42)\n#1 /fixture/nested-trace.php(8): outer(42)\n#2 {main}\n  thrown in /fixture/nested-trace.php on line 6"
    ));
}

#[test]
fn multiline_calls_use_the_named_callable_line_and_staticness() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nfunction captureNamed() {\n    return new Exception();\n}\n$named = captureNamed\n    (\n    );\nclass TraceFactory {\n    public $stored;\n    public static function captureStatic() {\n        return new Exception();\n    }\n    public function __construct() {\n        $this->stored = new Exception();\n    }\n}\n$static = TraceFactory\n    ::\n    captureStatic\n    ();\n$constructed = new\n    TraceFactory\n    ();\necho $named->getTraceAsString(), \"\\n--\\n\";\necho $static->getTraceAsString(), \"\\n--\\n\";\necho $constructed->stored->getTraceAsString();",
            "/fixture/multiline-call-trace.php",
            "/fixture",
        ),
        "#0 /fixture/multiline-call-trace.php(5): captureNamed()\n#1 {main}\n--\n#0 /fixture/multiline-call-trace.php(19): TraceFactory::captureStatic()\n#1 {main}\n--\n#0 /fixture/multiline-call-trace.php(22): TraceFactory->__construct()\n#1 {main}"
    );
}

#[test]
fn closure_creation_trace_uses_phps_public_closure_name() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\n$factory = function ($value) {\n    return new Exception('closure');\n};\n$stored = $factory(42);\necho $stored->getTraceAsString();",
            "/fixture/closure-trace.php",
            "/fixture",
        ),
        "#0 /fixture/closure-trace.php(5): {closure}(42)\n#1 {main}"
    );
}

#[test]
fn trace_arguments_hide_the_runtime_identity_suffix_of_anonymous_classes() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\n$factory = function ($value) {\n    return new Exception();\n};\n$stored = $factory(new class {});\necho $stored->getTraceAsString();",
            "/fixture/anonymous-trace-argument.php",
            "/fixture",
        ),
        "#0 /fixture/anonymous-trace-argument.php(5): {closure}(Object(class@anonymous))\n#1 {main}"
    );
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
