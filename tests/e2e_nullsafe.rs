mod common;

use common::{run_php, run_php_expect_error, run_php_with_source_context};

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
