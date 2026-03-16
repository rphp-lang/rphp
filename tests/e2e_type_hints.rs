/// Tests for parameter type hints
mod common;
use common::run_php;

// ── Basic scalar type hints ──

#[test]
fn test_int_type_hint_pass() {
    assert_eq!(run_php(r#"<?php
function add(int $a, int $b) { echo $a + $b; }
add(3, 4);
"#), "7");
}

#[test]
fn test_int_type_hint_fail() {
    assert_eq!(run_php(r#"<?php
function add(int $a, int $b) { echo $a + $b; }
try {
    add("hello", 4);
} catch (TypeError $e) {
    echo $e->getMessage();
}
"#).contains("must be of type int"), true);
}

#[test]
fn test_string_type_hint_pass() {
    assert_eq!(run_php(r#"<?php
function greet(string $name) { echo "Hello $name"; }
greet("world");
"#), "Hello world");
}

#[test]
fn test_string_type_hint_fail() {
    assert_eq!(run_php(r#"<?php
function greet(string $name) { echo "Hello $name"; }
try {
    greet(42);
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

#[test]
fn test_bool_type_hint_pass() {
    assert_eq!(run_php(r#"<?php
function check(bool $flag) { echo $flag ? "yes" : "no"; }
check(true);
"#), "yes");
}

#[test]
fn test_bool_type_hint_fail() {
    assert_eq!(run_php(r#"<?php
function check(bool $flag) { echo $flag ? "yes" : "no"; }
try {
    check(1);
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

#[test]
fn test_float_type_hint_pass() {
    assert_eq!(run_php(r#"<?php
function half(float $x) { echo $x / 2; }
half(10.0);
"#), "5");
}

#[test]
fn test_float_type_hint_accepts_int() {
    // PHP: float type hint accepts int values (widening)
    assert_eq!(run_php(r#"<?php
function half(float $x) { echo $x / 2; }
half(10);
"#), "5");
}

#[test]
fn test_float_type_hint_fail() {
    assert_eq!(run_php(r#"<?php
function half(float $x) { echo $x; }
try {
    half("abc");
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

#[test]
fn test_array_type_hint_pass() {
    assert_eq!(run_php(r#"<?php
function first(array $arr) { echo $arr[0]; }
first([10, 20, 30]);
"#), "10");
}

#[test]
fn test_array_type_hint_fail() {
    assert_eq!(run_php(r#"<?php
function first(array $arr) { echo $arr[0]; }
try {
    first("not array");
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

// ── Nullable type hints ──

#[test]
fn test_nullable_int_pass_int() {
    assert_eq!(run_php(r#"<?php
function show(?int $x) { echo $x ?? "null"; }
show(42);
"#), "42");
}

#[test]
fn test_nullable_int_pass_null() {
    assert_eq!(run_php(r#"<?php
function show(?int $x) { echo $x ?? "null"; }
show(null);
"#), "null");
}

#[test]
fn test_nullable_int_fail() {
    assert_eq!(run_php(r#"<?php
function show(?int $x) { echo $x; }
try {
    show("hello");
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

#[test]
fn test_nullable_string_pass() {
    assert_eq!(run_php(r#"<?php
function show(?string $x) { echo $x ?? "empty"; }
show(null);
echo " ";
show("hi");
"#), "empty hi");
}

// ── Class type hints ──

#[test]
fn test_class_type_hint_pass() {
    assert_eq!(run_php(r#"<?php
class Foo {
    public $val;
    public function __construct($v) { $this->val = $v; }
}
function show(Foo $f) { echo $f->val; }
show(new Foo("ok"));
"#), "ok");
}

#[test]
fn test_class_type_hint_fail() {
    assert_eq!(run_php(r#"<?php
class Foo {}
class Bar {}
function show(Foo $f) { echo "ok"; }
try {
    show(new Bar());
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

#[test]
fn test_class_type_hint_accepts_child() {
    assert_eq!(run_php(r#"<?php
class Animal {}
class Dog extends Animal {}
function show(Animal $a) { echo "ok"; }
show(new Dog());
"#), "ok");
}

#[test]
fn test_interface_type_hint() {
    assert_eq!(run_php(r#"<?php
interface Printable {
    public function display();
}
class Doc implements Printable {
    public function display() { echo "doc"; }
}
function show(Printable $p) { $p->display(); }
show(new Doc());
"#), "doc");
}

// ── Type hints with defaults ──

#[test]
fn test_type_hint_with_default() {
    assert_eq!(run_php(r#"<?php
function greet(string $name = "world") { echo "Hello $name"; }
greet();
"#), "Hello world");
}

#[test]
fn test_nullable_with_default_null() {
    assert_eq!(run_php(r#"<?php
function show(?int $x = null) { echo $x ?? "none"; }
show();
echo " ";
show(5);
"#), "none 5");
}

// ── Method type hints ──

#[test]
fn test_method_type_hint() {
    assert_eq!(run_php(r#"<?php
class Math {
    public function add(int $a, int $b) { echo $a + $b; }
}
$m = new Math();
$m->add(3, 4);
"#), "7");
}

#[test]
fn test_method_type_hint_fail() {
    assert_eq!(run_php(r#"<?php
class Math {
    public function add(int $a, int $b) { echo $a + $b; }
}
$m = new Math();
try {
    $m->add("x", 4);
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

// ── Multiple type-checked params ──

#[test]
fn test_multiple_typed_params() {
    assert_eq!(run_php(r#"<?php
function info(string $name, int $age, bool $active) {
    echo "$name $age " . ($active ? "yes" : "no");
}
info("Alice", 30, true);
"#), "Alice 30 yes");
}

#[test]
fn test_second_param_fails() {
    assert_eq!(run_php(r#"<?php
function info(string $name, int $age) { echo "$name $age"; }
try {
    info("Alice", "thirty");
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

// ── Closure type hints ──

#[test]
fn test_closure_type_hint() {
    assert_eq!(run_php(r#"<?php
$add = function(int $a, int $b) { return $a + $b; };
echo $add(3, 4);
"#), "7");
}

#[test]
fn test_closure_type_hint_fail() {
    assert_eq!(run_php(r#"<?php
$add = function(int $a, int $b) { return $a + $b; };
try {
    $add("x", 4);
} catch (TypeError $e) {
    echo "caught";
}
"#), "caught");
}

// ── Throwable type hint ──

#[test]
fn test_throwable_type_hint() {
    assert_eq!(run_php(r#"<?php
function handle(Throwable $e) { echo $e->getMessage(); }
handle(new Exception("test"));
"#), "test");
}
