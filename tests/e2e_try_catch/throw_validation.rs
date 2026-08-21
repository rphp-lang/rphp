// ============================================================
// throw validation — only Throwable objects allowed
// ============================================================

#[test]
fn test_throw_string_is_fatal() {
    assert_eq!(
        run_php(r#"<?php try { throw "boom"; } catch (Error $error) { echo $error->getMessage(); }"#),
        "Can only throw objects"
    );
}

#[test]
fn test_throw_integer_is_fatal() {
    assert_eq!(
        run_php(r#"<?php try { throw 42; } catch (Error $error) { echo $error->getMessage(); }"#),
        "Can only throw objects"
    );
}

#[test]
fn test_throw_non_throwable_object_is_fatal() {
    assert_eq!(
        run_php(
        r#"<?php
class Foo {}
try { throw new Foo(); } catch (Error $error) { echo $error->getMessage(); }
"#,
        ),
        "Cannot throw objects that do not implement Throwable"
    );
}

#[test]
fn test_throw_null_is_fatal() {
    assert_eq!(
        run_php(r#"<?php try { throw null; } catch (Error $error) { echo $error->getMessage(); }"#),
        "Can only throw objects"
    );
}

#[test]
fn non_instantiable_classes_raise_catchable_errors_at_new() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
interface Contract {}
abstract class Template {}
foreach ([Contract::class, Template::class, Generator::class] as $class) {
    try { new $class; } catch (Error $error) {
        echo $error->getMessage(), '@', $error->getLine(), "\n";
    }
}
"#,
            "/fixture/non-instantiable.php",
            "/fixture",
        ),
        "Cannot instantiate interface Contract@5\nCannot instantiate abstract class Template@5\nThe \"Generator\" class is reserved for internal use and cannot be manually instantiated@5\n"
    );
}

#[test]
fn missing_classes_raise_located_catchable_errors_at_new() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
foreach (['MissingLiteral', 'MissingDynamic'] as $class) {
    try {
        if ($class === 'MissingLiteral') { new MissingLiteral; }
        else { new $class; }
    } catch (Error $error) {
        echo $error->getMessage(), '@', $error->getFile(), ':', $error->getLine(), "\n";
    }
}
"#,
            "/fixture/missing-class.php",
            "/fixture",
        ),
        "Class \"MissingLiteral\" not found@/fixture/missing-class.php:4\nClass \"MissingDynamic\" not found@/fixture/missing-class.php:5\n"
    );
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
    let out = run_php(
        r#"<?php
try {
    throw new Exception("msg", 1, null, "extra");
} catch (ArgumentCountError $e) {
    echo $e->getMessage();
}
"#,
    );
    assert_eq!(
        out,
        "Exception::__construct() expects at most 3 arguments, 4 given"
    );
}

#[test]
fn test_getmessage_extra_args_too_many() {
    // getMessage("unused") — getMessage takes 0 public args, so 1 is too many
    let out = run_php(
        r#"<?php
try {
    throw new Exception("hello");
} catch (Exception $e) {
    try {
        echo $e->getMessage("unused");
    } catch (ArgumentCountError $error) {
        echo $error->getMessage();
    }
}
"#,
    );
    assert_eq!(
        out,
        "Exception::getmessage() expects exactly 0 arguments, 1 given"
    );
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
fn exception_ignore_args_omits_arguments_when_the_trace_is_created() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nfunction captureIgnored($secret) { return new Exception(); }\nini_set('zend.exception_ignore_args', '1');\n$ignored = captureIgnored('hidden');\nini_set('zend.exception_ignore_args', '0');\nvar_export($ignored->getTrace());\necho \"\\n\", $ignored->getTraceAsString();",
            "/fixture/ignore-args.php",
            "/fixture",
        ),
        "array (\n  0 => array (\n  'file' => '/fixture/ignore-args.php',\n  'line' => 4,\n  'function' => 'captureIgnored',\n),\n)\n#0 /fixture/ignore-args.php(4): captureIgnored()\n#1 {main}"
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
fn undefined_named_calls_retain_the_origin_across_tostring_dispatch() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nfunction failNamed(): void {\n    missingNamedTarget();\n}\nclass FailString {\n    public function __toString(): string {\n        missingStringTarget();\n    }\n}\ntry {\n    failNamed();\n} catch (Throwable $error) {\n    echo 'named=', $error->getFile(), ':', $error->getLine(), \"\\n\",\n        $error->getTraceAsString(), \"\\n--\\n\";\n}\ntry {\n    echo new FailString;\n} catch (Throwable $error) {\n    echo 'string=', $error->getFile(), ':', $error->getLine(), \"\\n\",\n        $error->getTraceAsString();\n}",
            "/fixture/undefined-call-origin.php",
            "/fixture",
        ),
        "named=/fixture/undefined-call-origin.php:3\n#0 /fixture/undefined-call-origin.php(11): failNamed()\n#1 {main}\n--\nstring=/fixture/undefined-call-origin.php:7\n#0 /fixture/undefined-call-origin.php(17): FailString->__toString()\n#1 {main}"
    );
}

#[test]
fn bound_closure_property_modify_errors_keep_the_origin_and_closure_trace_name() {
    let error = run_php_expect_error_with_source_context(
        "<?php\nclass BoundTrace {\n    private int $value = 1;\n    public function make() {\n        return function() { return ++$this->value; };\n    }\n}\n$closure = (new BoundTrace)->make();\n$bound = $closure->bindTo(new BoundTrace, null);\n$bound();",
        "/fixture/bound-closure-trace.php",
        "/fixture",
    );

    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message)
            if message == "Uncaught Error: Cannot access private property BoundTrace::$value in /fixture/bound-closure-trace.php:5\nStack trace:\n#0 /fixture/bound-closure-trace.php(10): Closure->{closure:BoundTrace::make():5}()\n#1 {main}\n  thrown in /fixture/bound-closure-trace.php on line 5"
    ));
}

#[test]
fn static_property_errors_keep_each_operation_source_line() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nclass StaticOrigin {\n    private static int $secret = 1;\n    public static int $typed;\n}\n$probes = [\n    function() { return StaticOrigin::$missing; },\n    function() { return StaticOrigin::$secret; },\n    function() { return StaticOrigin::$typed; },\n    function() { StaticOrigin::$missing = 1; },\n    function() { unset(StaticOrigin::$missing); },\n];\nforeach ($probes as $probe) {\n    try { $probe(); } catch (Throwable $error) {\n        echo get_class($error), '|', $error->getMessage(), '|', $error->getFile(), ':', $error->getLine(), \"\\n\";\n    }\n}",
            "/fixture/static-property-origin.php",
            "/fixture",
        ),
        "Error|Access to undeclared static property StaticOrigin::$missing|/fixture/static-property-origin.php:7\nError|Cannot access private property StaticOrigin::$secret|/fixture/static-property-origin.php:8\nError|Typed static property StaticOrigin::$typed must not be accessed before initialization|/fixture/static-property-origin.php:9\nError|Access to undeclared static property StaticOrigin::$missing|/fixture/static-property-origin.php:10\nError|Attempt to unset static property StaticOrigin::$missing|/fixture/static-property-origin.php:11\n"
    );
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
        "#0 /fixture/closure-trace.php(5): {closure:/fixture/closure-trace.php:2}(42)\n#1 {main}"
    );
}

#[test]
fn nested_closure_trace_names_retain_the_public_parent_location() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\n$outer = function () {\n    return function () { return new Exception(); };\n};\n$inner = $outer();\necho $inner()->getTraceAsString();",
            "/fixture/nested-closure-trace.php",
            "/fixture",
        ),
        "#0 /fixture/nested-closure-trace.php(6): {closure:{closure:/fixture/nested-closure-trace.php:2}:3}()\n#1 {main}"
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
        "#0 /fixture/anonymous-trace-argument.php(5): {closure:/fixture/anonymous-trace-argument.php:2}(Object(class@anonymous))\n#1 {main}"
    );
}

#[test]
fn throwable_trace_strings_escape_bytes_like_php() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nfunction capture($value) { return (new Exception())->getTraceAsString(); }\necho capture(\"a\\\\b\\nž\");",
            "/fixture/trace-string-bytes.php",
            "/fixture",
        ),
        "#0 /fixture/trace-string-bytes.php(3): capture('a\\\\b\\n\\xC5\\xBE')\n#1 {main}"
    );
}

#[test]
fn trace_callables_hide_the_runtime_identity_suffix_of_anonymous_classes() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\n$object = new class {\n    public $trace;\n    public function __construct() { $this->trace = new Exception(); }\n};\necho $object->trace->getTraceAsString();",
            "/fixture/anonymous-trace-callable.php",
            "/fixture",
        ),
        "#0 /fixture/anonymous-trace-callable.php(2): class@anonymous->__construct()\n#1 {main}"
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
fn uncaught_throwable_renders_the_complete_previous_chain_oldest_first() {
    let error = run_php_expect_error_with_source_context(
        "<?php\n$root = new Error('root');\n$middle = new Exception('', 0, $root);\nthrow new RuntimeException('outer', 0, $middle);",
        "/fixture/previous-chain.php",
        "/fixture",
    );

    assert!(matches!(
        error,
        rphp::vm::execute::VmError::Fatal(message)
            if message == "Uncaught Error: root in /fixture/previous-chain.php:2\nStack trace:\n#0 {main}\n\nNext Exception in /fixture/previous-chain.php:3\nStack trace:\n#0 {main}\n\nNext RuntimeException: outer in /fixture/previous-chain.php:4\nStack trace:\n#0 {main}\n  thrown in /fixture/previous-chain.php on line 4"
    ));
}

#[test]
fn throwable_string_rendering_uses_the_stored_origin_trace_and_previous_chain() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\n$root = new Error('root');\n$outer = new RuntimeException('', 0, $root);\necho $outer;",
            "/fixture/throwable-string.php",
            "/fixture",
        ),
        "Error: root in /fixture/throwable-string.php:2\nStack trace:\n#0 {main}\n\nNext RuntimeException in /fixture/throwable-string.php:3\nStack trace:\n#0 {main}"
    );
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
