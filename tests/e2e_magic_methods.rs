mod common;
use common::run_php;

#[test]
fn test_tostring_echo() {
    assert_eq!(
        run_php(
            r#"<?php
class Money {
    private int $cents;
    public function __construct(int $cents) {
        $this->cents = $cents;
    }
    public function __toString(): string {
        return "USD:" . $this->cents;
    }
}
$m = new Money(1550);
echo $m;
"#
        ),
        "USD:1550"
    );
}

#[test]
fn test_tostring_concat() {
    assert_eq!(
        run_php(
            r#"<?php
class Tag {
    private string $name;
    public function __construct(string $name) { $this->name = $name; }
    public function __toString(): string { return "<" . $this->name . ">"; }
}
$t = new Tag("div");
echo "HTML: " . $t;
"#
        ),
        "HTML: <div>"
    );
}

#[test]
fn test_get_set() {
    assert_eq!(
        run_php(
            r#"<?php
class Bag {
    private $data;
    public function __construct() {
        $this->data = [];
    }
    public function __get($name) {
        return $this->data[$name] ?? "none";
    }
    public function __set($name, $value) {
        $this->data[$name] = $value;
    }
}
$b = new Bag();
$b->color = "red";
echo $b->color;
"#
        ),
        "red"
    );
}

#[test]
fn test_get_undefined_property() {
    assert_eq!(
        run_php(
            r#"<?php
class Flex {
    public function __get($name) {
        return "default_" . $name;
    }
}
$f = new Flex();
echo $f->whatever;
"#
        ),
        "default_whatever"
    );
}

#[test]
fn recursive_get_is_guarded_per_object_and_property() {
    assert_eq!(
        run_php(
            r#"<?php
class RecursiveGet {
    public function __get($name) {
        echo "get:$name\n";
        if ($name === 'first') {
            var_dump($this->{$name . ''});
            var_dump($this->second);
        }
    }
}
$object = new RecursiveGet();
var_dump($object->first);
"#
        ),
        "get:first\nNULL\nget:second\nNULL\nNULL\n"
    );
}

#[test]
fn recursive_set_writes_dynamic_property_without_reentering_setter() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class RecursiveSet {
    public function __set($name, $value) {
        echo "set:$name\n";
        $this->$name = $value;
    }
}
$object = new RecursiveSet();
$object->answer = 42;
var_dump($object->answer);
"#
        ),
        "set:answer\nint(42)\n"
    );
}

#[test]
fn recursive_isset_is_guarded_without_suppressing_get() {
    assert_eq!(
        run_php(
            r#"<?php
class RecursiveIsset {
    public function __isset($name) {
        echo "isset:$name\n";
        var_dump(isset($this->$name));
        return true;
    }
    public function __get($name) {
        echo "get:$name\n";
        return 7;
    }
}
$object = new RecursiveIsset();
var_dump(isset($object->value));
"#
        ),
        "isset:value\nbool(false)\nbool(true)\n"
    );
}

#[test]
fn recursive_unset_is_guarded_per_object_and_property() {
    assert_eq!(
        run_php(
            r#"<?php
class RecursiveUnset {
    public function __unset($name) {
        echo "unset:$name\n";
        unset($this->$name);
        if ($name === 'first') {
            unset($this->second);
        }
    }
}
$object = new RecursiveUnset();
unset($object->first);
"#
        ),
        "unset:first\nunset:second\n"
    );
}

#[test]
fn magic_property_guard_is_released_after_exception() {
    assert_eq!(
        run_php(
            r#"<?php
class ThrowingGet {
    public function __get($name) {
        echo "get:$name\n";
        throw new Exception('boom');
    }
}
$object = new ThrowingGet();
for ($attempt = 0; $attempt < 2; $attempt++) {
    try { $object->value; } catch (Exception $error) {}
}
"#
        ),
        "get:value\nget:value\n"
    );
}

#[test]
fn inaccessible_declared_properties_use_magic_fallback() {
    assert_eq!(
        run_php(
            r#"<?php
class MagicVisibility {
    protected $hidden;
    public function __get($name) { echo "get:$name\n"; return $this->$name; }
    public function __set($name, $value) { echo "set:$name\n"; $this->$name = $value; }
}
$object = new MagicVisibility();
$object->hidden = 42;
var_dump($object->hidden);
"#
        ),
        "set:hidden\nget:hidden\nint(42)\n"
    );
}

#[test]
fn recursive_magic_access_to_nul_property_throws_catchable_error() {
    assert_eq!(
        run_php(
            r#"<?php
class NulProperty {
    public function __set($name, $value) { $this->$name = $value; }
    public function __get($name) { return $this->$name; }
}
$object = new NulProperty();
foreach (['write', 'read'] as $operation) {
    try {
        if ($operation === 'write') { $object->{"\0"} = 2; }
        else { $object->{"\0"}; }
    } catch (Error $error) { echo $error->getMessage(), "\n"; }
}
"#
        ),
        "Cannot access property starting with \"\\0\"\nCannot access property starting with \"\\0\"\n"
    );
}

#[test]
fn direct_nul_property_access_throws_without_magic_methods() {
    assert_eq!(
        run_php(
            r#"<?php
$object = new stdClass();
foreach (['write', 'read'] as $operation) {
    try {
        if ($operation === 'write') { $object->{"\0"} = 2; }
        else { $object->{"\0"}; }
    } catch (Error $error) { echo $error->getMessage(), "\n"; }
}
"#
        ),
        "Cannot access property starting with \"\\0\"\nCannot access property starting with \"\\0\"\n"
    );
}

#[test]
fn test_invoke() {
    assert_eq!(
        run_php(
            r#"<?php
class Multiplier {
    private int $factor;
    public function __construct(int $factor) { $this->factor = $factor; }
    public function __invoke(int $x): int { return $this->factor * $x; }
}
$double = new Multiplier(2);
echo $double(21);
"#
        ),
        "42"
    );
}

#[test]
fn test_invoke_with_closure_like_usage() {
    assert_eq!(
        run_php(
            r#"<?php
class Greeter {
    private string $greeting;
    public function __construct(string $greeting) { $this->greeting = $greeting; }
    public function __invoke(string $name): string { return $this->greeting . " " . $name; }
}
$hi = new Greeter("Hello");
echo $hi("World");
"#
        ),
        "Hello World"
    );
}
