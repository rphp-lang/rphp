mod common;

use common::{
    run_php, run_php_expect_error, run_php_expect_error_with_source_context,
    run_php_with_source_context,
};

#[test]
fn nullsafe_method_on_null() {
    let out = run_php(
        r#"<?php
class Foo {
    public function greet() { return "hello"; }
}
$obj = null;
$result = $obj?->greet();
echo var_export($result, true);
"#,
    );
    assert_eq!(out, "NULL");
}

#[test]
fn nullsafe_method_on_object() {
    let out = run_php(
        r#"<?php
class Foo {
    public function greet() { return "hello"; }
}
$obj = new Foo();
$result = $obj?->greet();
echo $result;
"#,
    );
    assert_eq!(out, "hello");
}

#[test]
fn nullsafe_property_on_null() {
    let out = run_php(
        r#"<?php
class Foo {
    public $name = "world";
}
$obj = null;
$result = $obj?->name;
echo var_export($result, true);
"#,
    );
    assert_eq!(out, "NULL");
}

#[test]
fn nullsafe_property_on_object() {
    let out = run_php(
        r#"<?php
class Foo {
    public $name = "world";
}
$obj = new Foo();
$result = $obj?->name;
echo $result;
"#,
    );
    assert_eq!(out, "world");
}

#[test]
fn nullsafe_chained_property() {
    let out = run_php(
        r#"<?php
class Bar {
    public $value = 42;
}
class Foo {
    public $bar;
    public function __construct() {
        $this->bar = new Bar();
    }
}
$obj = new Foo();
$result = $obj?->bar?->value;
echo $result;
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn nullsafe_chained_first_null() {
    let out = run_php(
        r#"<?php
class Bar {
    public $value = 42;
}
class Foo {
    public $bar;
}
$obj = null;
$result = $obj?->bar?->value;
echo var_export($result, true);
"#,
    );
    assert_eq!(out, "NULL");
}

#[test]
fn nullsafe_chained_middle_null() {
    let out = run_php(
        r#"<?php
class Bar {
    public $value = 42;
}
class Foo {
    public $bar;
}
$obj = new Foo();
$result = $obj?->bar?->value;
echo var_export($result, true);
"#,
    );
    assert_eq!(out, "NULL");
}

#[test]
fn nullsafe_mixed_method_property() {
    let out = run_php(
        r#"<?php
class Inner {
    public $data = "found";
}
class Outer {
    public function getInner() { return new Inner(); }
}
$obj = new Outer();
$result = $obj?->getInner()?->data;
echo $result;
"#,
    );
    assert_eq!(out, "found");
}

#[test]
fn nullsafe_mixed_method_returns_null() {
    let out = run_php(
        r#"<?php
class Outer {
    public function getInner() { return null; }
}
$obj = new Outer();
$result = $obj?->getInner()?->data;
echo var_export($result, true);
"#,
    );
    assert_eq!(out, "NULL");
}

#[test]
fn nullsafe_in_echo() {
    let out = run_php(
        r#"<?php
class Foo {
    public $name = "test";
}
$obj = null;
echo $obj?->name;
echo "done";
"#,
    );
    assert_eq!(out, "done");
}

#[test]
fn nullsafe_with_regular_arrow() {
    // Mix of regular -> and ?-> in same expression
    let out = run_php(
        r#"<?php
class Bar {
    public $value = "ok";
}
class Foo {
    public $bar;
    public function __construct() {
        $this->bar = new Bar();
    }
}
$obj = new Foo();
$result = $obj->bar?->value;
echo $result;
"#,
    );
    assert_eq!(out, "ok");
}

#[test]
fn nullsafe_property_on_scalar_warns_null() {
    // PHP: ?-> property access on scalar emits warning and returns null
    let out = run_php(
        r#"<?php
$x = 42;
$result = $x?->foo;
echo var_export($result, true);
echo "done";
"#,
    );
    assert!(out.contains("Warning"), "Expected warning, got: {}", out);
    assert!(
        out.contains("NULLdone"),
        "Expected null result, got: {}",
        out
    );
}

#[test]
fn nullsafe_property_on_string_warns_null() {
    let out = run_php(
        r#"<?php
$x = "hello";
$result = $x?->length;
echo var_export($result, true);
echo "done";
"#,
    );
    assert!(out.contains("Warning"), "Expected warning, got: {}", out);
    assert!(
        out.contains("NULLdone"),
        "Expected null result, got: {}",
        out
    );
}

#[test]
fn nullsafe_method_on_scalar_fatals() {
    // PHP: ?-> method call on scalar is a fatal error
    let err = run_php_expect_error(
        r#"<?php
$x = 42;
$result = $x?->toString();
"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("non-object") || msg.contains("member") || msg.contains("method"),
        "Expected error about non-object method call, got: {}",
        msg
    );
}

#[test]
fn nullsafe_scalar_method_errors_are_catchable_and_type_specific() {
    let out = run_php_with_source_context(
        r#"<?php
foreach ([false, [], 0, 0.0, ''] as $value) {
    try {
        $value?->missing();
    } catch (Error $error) {
        echo $error->getMessage(), "|", $error->getLine(), "\n";
    }
}
"#,
        "/virtual/nullsafe-scalar.php",
        "/virtual",
    );
    assert_eq!(
        out,
        concat!(
            "Call to a member function missing() on bool|4\n",
            "Call to a member function missing() on array|4\n",
            "Call to a member function missing() on int|4\n",
            "Call to a member function missing() on float|4\n",
            "Call to a member function missing() on string|4\n",
        )
    );
}

#[test]
fn ordinary_non_object_method_errors_use_php_82_type_names() {
    let out = run_php_with_source_context(
        r#"<?php
foreach ([null, false, true, [], 1, 1.0, 'x'] as $value) {
    try {
        $value->missing();
    } catch (Error $error) {
        echo $error->getMessage(), "|", $error->getLine(), "\n";
    }
}
"#,
        "/virtual/non-object-method.php",
        "/virtual",
    );
    assert_eq!(
        out,
        concat!(
            "Call to a member function missing() on null|4\n",
            "Call to a member function missing() on bool|4\n",
            "Call to a member function missing() on bool|4\n",
            "Call to a member function missing() on array|4\n",
            "Call to a member function missing() on int|4\n",
            "Call to a member function missing() on float|4\n",
            "Call to a member function missing() on string|4\n",
        )
    );
}

#[test]
fn nullsafe_short_circuit_spans_regular_postfixes_but_not_unrelated_arguments() {
    let out = run_php(
        r#"<?php
class ChainProbe {
    public function returnsNull() { return null; }
    public function accept($value) { echo "accept|"; return $value; }
}
$null = null;
var_dump($null?->returnsNull()->missing(expensive())[0]->property);
$probe = new ChainProbe;
var_dump($probe->accept($null?->returnsNull()));
"#,
    );
    assert_eq!(out, concat!("NULL\n", "accept|NULL\n"));

    let error = run_php_expect_error(
        r#"<?php
class ChainProbe { public function returnsNull() { return null; } }
$probe = new ChainProbe;
$probe?->returnsNull()->missing();
"#,
    );
    assert!(format!("{error:?}").contains("missing"));
}

#[test]
fn nullsafe_short_circuit_spans_dynamic_static_calls() {
    let out = run_php(
        r#"<?php
class StaticTarget {
    public static function ping($value) { echo "ping:$value|"; }
}
class StaticProvider {
    public function target() { return StaticTarget::class; }
}
function methodName() { echo "method|"; return 'ping'; }
$null = null;
var_dump($null?->target()::ping(argumentMustNotRun()));
var_dump($null?->target()::{methodName()}(argumentMustNotRun()));
var_dump($null?->target()::$undefinedMethod(argumentMustNotRun()));
$provider = new StaticProvider;
var_dump($provider?->target()::ping('named'));
var_dump($provider?->target()::{methodName()}('dynamic'));
$method = 'ping';
var_dump($provider?->target()::$method('variable'));
"#,
    );
    assert_eq!(
        out,
        concat!(
            "NULL\n",
            "NULL\n",
            "NULL\n",
            "ping:named|NULL\n",
            "method|ping:dynamic|NULL\n",
            "ping:variable|NULL\n",
        )
    );
}

#[test]
fn nullsafe_write_contexts_fail_during_compilation_with_source_location() {
    for expression in [
        "$foo?->bar = sideEffect();",
        "$foo?->bar += sideEffect();",
        "++$foo?->bar;",
        "$foo?->bar++;",
        "$foo?->bar ??= sideEffect();",
        "$foo?->bar->baz = sideEffect();",
        "$foo?->bar[0]++;",
    ] {
        let source = format!("<?php\n$foo = null;\n{expression}\n");
        let error = run_php_expect_error_with_source_context(
            &source,
            "/virtual/nullsafe-write.php",
            "/virtual",
        );
        assert_eq!(
            format!("{error:?}"),
            "Fatal(\"Can't use nullsafe operator in write context in /virtual/nullsafe-write.php on line 3\")"
        );
    }
}

#[test]
fn nullsafe_reference_unset_and_foreach_targets_fail_during_compilation() {
    for (expression, message) in [
        (
            "$ref =& $foo?->bar;",
            "Cannot take reference of a nullsafe chain",
        ),
        (
            "$ref =& $foo?->bar->baz;",
            "Cannot take reference of a nullsafe chain",
        ),
        (
            "$ref =& $foo?->bar();",
            "Cannot take reference of a nullsafe chain",
        ),
        (
            "$ref =& $foo?->bar()::baz();",
            "Cannot take reference of a nullsafe chain",
        ),
        (
            "unset($foo?->bar->baz);",
            "Can't use nullsafe operator in write context",
        ),
        (
            "foreach ([1, 2] as $foo?->bar) { sideEffect(); }",
            "Can't use nullsafe operator in write context",
        ),
    ] {
        let source = format!("<?php\n$foo = null;\n{expression}\n");
        let error = run_php_expect_error_with_source_context(
            &source,
            "/virtual/nullsafe-forbidden.php",
            "/virtual",
        );
        assert_eq!(
            format!("{error:?}"),
            format!("Fatal(\"{message} in /virtual/nullsafe-forbidden.php on line 3\")")
        );
    }
}

#[test]
fn nullsafe_call_arguments_are_values_but_never_referenceable() {
    let output = run_php(
        r#"<?php
function mutate(&$slot) { $slot = 'changed'; }
function observe($value) { var_dump($value); }
function receiver($kind) {
    echo "receiver:$kind\n";
    return $kind === 'object' ? (object) ['slot' => 'original'] : null;
}

foreach (['null', 'object'] as $kind) {
    try {
        mutate(receiver($kind)?->slot);
    } catch (Error $error) {
        echo $error->getMessage(), "\n";
    }
}

$callback = 'mutate';
try {
    $callback(slot: receiver('named')?->slot);
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}

observe(receiver('value')?->slot);
"#,
    );

    assert_eq!(
        output,
        concat!(
            "receiver:null\n",
            "mutate(): Argument #1 ($slot) cannot be passed by reference\n",
            "receiver:object\n",
            "mutate(): Argument #1 ($slot) cannot be passed by reference\n",
            "receiver:named\n",
            "mutate(): Argument #1 ($slot) cannot be passed by reference\n",
            "receiver:value\n",
            "NULL\n",
        )
    );
}

#[test]
fn nullsafe_chains_cannot_be_returned_from_reference_declarations() {
    for source in [
        "<?php\necho 'unreachable';\nfunction &pick($value) { return $value?->slot; }\n",
        "<?php\necho 'unreachable';\nclass Picker { function &pick($value) { return $value->inner?->slot; } }\n",
        "<?php\necho 'unreachable';\n$pick = function &($value) { return $value?->slot; };\n",
    ] {
        let error = run_php_expect_error_with_source_context(
            source,
            "/virtual/nullsafe-reference-return.php",
            "/virtual",
        );
        assert_eq!(
            format!("{error:?}"),
            "Fatal(\"Cannot take reference of a nullsafe chain in /virtual/nullsafe-reference-return.php on line 3\")"
        );
    }
}

#[test]
fn nullsafe_destructuring_targets_fail_before_execution() {
    for (assignment, message) in [
        (
            "[$foo?->bar] = sideEffect();",
            "Assignments can only happen to writable values",
        ),
        (
            "list($foo?->bar->baz) = sideEffect();",
            "Assignments can only happen to writable values",
        ),
        (
            "[[$foo->bar?->baz]] = sideEffect();",
            "Assignments can only happen to writable values",
        ),
        (
            "[$foo?->bar[]] = sideEffect();",
            "Assignments can only happen to writable values",
        ),
        (
            "[&$foo?->bar] = sideEffect();",
            "Cannot assign reference to non referenceable value",
        ),
    ] {
        let source = format!("<?php\necho 'unreachable';\n{assignment}\n");
        let error = run_php_expect_error_with_source_context(
            &source,
            "/virtual/nullsafe-destructuring.php",
            "/virtual",
        );
        assert_eq!(
            format!("{error:?}"),
            format!("Fatal(\"{message} in /virtual/nullsafe-destructuring.php on line 3\")")
        );
    }
}

#[test]
fn nullsafe_by_reference_foreach_uses_a_detached_outer_snapshot() {
    let output = run_php(
        r#"<?php
set_error_handler(function($level, $message) { echo "handled:$message\n"; });
$null = null;
foreach ($null?->items as &$value) {}

$box = (object) ['items' => [1, 2]];
foreach ($box?->items as &$value) { $value += 10; }
unset($value);
var_dump($box->items);

$shared = 5;
$box->items = [&$shared];
foreach ($box?->items as &$value) { $value++; }
unset($value);
var_dump($shared, $box->items);

$calls = 0;
function selectBox($box) { global $calls; $calls++; return $box; }
foreach (selectBox($box)?->items as &$value) { $value++; }
unset($value);
var_dump($calls, $shared);
"#,
    );

    assert_eq!(
        output,
        concat!(
            "handled:foreach() argument must be of type array|object, null given\n",
            "array(2) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(2)\n}\n",
            "int(6)\n",
            "array(1) {\n  [0]=>\n  &int(6)\n}\n",
            "int(1)\n",
            "int(7)\n",
        )
    );
}

#[test]
fn braced_dynamic_nullsafe_property_skips_or_evaluates_its_name_once() {
    let out = run_php(
        r#"<?php
$calls = 0;
function memberName() {
    global $calls;
    $calls++;
    return 'value';
}
class DynamicNullsafeBox { public $value = 'ok'; }
class StringPropertyName {
    public function __toString() { echo "convert\n"; return 'value'; }
}
class InvalidPropertyName {}
class RebindingPropertyName {
    public function __toString() {
        global $box;
        $box = null;
        return 'value';
    }
}
$null = null;
var_dump($null?->{memberName()});
var_dump($calls);
$box = new DynamicNullsafeBox;
var_dump($box?->{memberName()});
var_dump($calls);
var_dump($box->{new StringPropertyName});
try {
    $box->{new InvalidPropertyName};
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
$box = new DynamicNullsafeBox;
var_dump($box->{new RebindingPropertyName});
var_dump($box);
$dynamic = new stdClass;
$dynamic->{90} = 'ninety';
var_dump($dynamic->{90});
"#,
    );
    assert_eq!(
        out,
        concat!(
            "NULL\n",
            "int(0)\n",
            "string(2) \"ok\"\n",
            "int(1)\n",
            "convert\n",
            "string(2) \"ok\"\n",
            "Object of class InvalidPropertyName could not be converted to string\n",
            "string(2) \"ok\"\n",
            "NULL\n",
            "string(6) \"ninety\"\n",
        )
    );
}
