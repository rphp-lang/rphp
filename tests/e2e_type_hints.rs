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

// ── Return type hints ──

#[test]
fn test_return_type_int() {
    assert_eq!(run_php(r#"<?php
function add(int $a, int $b): int { return $a + $b; }
echo add(2, 3);
"#), "5");
}

#[test]
fn test_return_type_string() {
    assert_eq!(run_php(r#"<?php
function greet(string $name): string { return "Hello " . $name; }
echo greet("PHP");
"#), "Hello PHP");
}

#[test]
fn test_return_type_bool() {
    assert_eq!(run_php(r#"<?php
function isPositive(int $n): bool { return $n > 0; }
echo isPositive(5) ? "yes" : "no";
"#), "yes");
}

#[test]
fn test_return_type_array() {
    assert_eq!(run_php(r#"<?php
function makeArr(): array { return [1, 2, 3]; }
echo count(makeArr());
"#), "3");
}

#[test]
fn test_return_type_float() {
    assert_eq!(run_php(r#"<?php
function half(int $n): float { return $n / 2; }
echo half(7);
"#), "3.5");
}

#[test]
fn test_return_type_mismatch_throws() {
    assert_eq!(run_php(r#"<?php
function bad(): int { return "hello"; }
try {
    bad();
} catch (TypeError $e) {
    echo $e->getMessage();
}
"#), "Return value must be of type int, string returned");
}

#[test]
fn test_return_type_nullable_pass_null() {
    assert_eq!(run_php(r#"<?php
function maybe(): ?int { return null; }
echo maybe() === null ? "null" : "not null";
"#), "null");
}

#[test]
fn test_return_type_nullable_pass_value() {
    assert_eq!(run_php(r#"<?php
function maybe(): ?int { return 42; }
echo maybe();
"#), "42");
}

#[test]
fn test_return_type_nullable_fail() {
    assert_eq!(run_php(r#"<?php
function maybe(): ?int { return "oops"; }
try { maybe(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

// ── Void return type ──

#[test]
fn test_void_bare_return() {
    assert_eq!(run_php(r#"<?php
function doStuff(): void { echo "done"; return; }
doStuff();
"#), "done");
}

#[test]
fn test_void_implicit_return() {
    assert_eq!(run_php(r#"<?php
function doStuff(): void { echo "done"; }
doStuff();
"#), "done");
}

#[test]
fn test_void_return_value_error() {
    assert_eq!(run_php(r#"<?php
function bad(): void { return 42; }
try { bad(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

// ── Mixed return type ──

#[test]
fn test_mixed_return_int() {
    assert_eq!(run_php(r#"<?php
function f(): mixed { return 42; }
echo f();
"#), "42");
}

#[test]
fn test_mixed_return_string() {
    assert_eq!(run_php(r#"<?php
function f(): mixed { return "hello"; }
echo f();
"#), "hello");
}

#[test]
fn test_mixed_return_null() {
    assert_eq!(run_php(r#"<?php
function f(): mixed { return null; }
echo f() === null ? "null" : "other";
"#), "null");
}

// ── Never return type ──

#[test]
fn test_never_throws_ok() {
    assert_eq!(run_php(r#"<?php
function fail(): never { throw new Exception("bye"); }
try { fail(); } catch (Exception $e) { echo $e->getMessage(); }
"#), "bye");
}

#[test]
fn test_never_return_error() {
    assert_eq!(run_php(r#"<?php
function bad(): never { return 42; }
try { bad(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

// ── Union types ──

#[test]
fn test_union_return_int_ok() {
    assert_eq!(run_php(r#"<?php
function f(): int|string { return 42; }
echo f();
"#), "42");
}

#[test]
fn test_union_return_string_ok() {
    assert_eq!(run_php(r#"<?php
function f(): int|string { return "hello"; }
echo f();
"#), "hello");
}

#[test]
fn test_union_return_fail() {
    assert_eq!(run_php(r#"<?php
function f(): int|string { return [1,2]; }
try { f(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_union_param_types() {
    assert_eq!(run_php(r#"<?php
function show(int|string $x): void { echo $x; }
show(42);
echo " ";
show("hi");
"#), "42 hi");
}

#[test]
fn test_union_param_fail() {
    assert_eq!(run_php(r#"<?php
function show(int|string $x): void { echo $x; }
try { show([1]); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_union_three_types() {
    assert_eq!(run_php(r#"<?php
function f(int|string|bool $x): void { echo $x; }
f(42);
f("hi");
f(true);
"#), "42hi1");
}

// ── Class return type hints ──

#[test]
fn test_return_type_class() {
    assert_eq!(run_php(r#"<?php
class Foo { public $x; public function __construct($x) { $this->x = $x; } }
function makeFoo(): Foo { return new Foo(42); }
$f = makeFoo();
echo $f->x;
"#), "42");
}

#[test]
fn test_return_type_class_fail() {
    assert_eq!(run_php(r#"<?php
class Foo {}
function makeFoo(): Foo { return "not an object"; }
try { makeFoo(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

// ── Method return types ──

#[test]
fn test_method_return_type() {
    assert_eq!(run_php(r#"<?php
class Calc {
    public function add(int $a, int $b): int { return $a + $b; }
}
$c = new Calc();
echo $c->add(3, 4);
"#), "7");
}

#[test]
fn test_method_return_type_fail() {
    assert_eq!(run_php(r#"<?php
class Calc {
    public function bad(): int { return "nope"; }
}
$c = new Calc();
try { $c->bad(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

// ── Closure & arrow function return types ──

#[test]
fn test_closure_return_type() {
    assert_eq!(run_php(r#"<?php
$f = function(int $x): int { return $x * 2; };
echo $f(5);
"#), "10");
}

#[test]
fn test_closure_return_type_fail() {
    assert_eq!(run_php(r#"<?php
$f = function(): int { return "bad"; };
try { $f(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_arrow_fn_return_type() {
    assert_eq!(run_php(r#"<?php
$f = fn(int $x): int => $x * 3;
echo $f(4);
"#), "12");
}

#[test]
fn test_arrow_fn_return_type_fail() {
    assert_eq!(run_php(r#"<?php
$f = fn(): int => "bad";
try { $f(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

// ── Recovery after return type error ──

#[test]
fn test_return_type_error_recovery() {
    assert_eq!(run_php(r#"<?php
function good(): int { return 42; }
function bad(): int { return "x"; }
try { bad(); } catch (TypeError $e) { echo "caught "; }
echo good();
"#), "caught 42");
}

#[test]
fn test_exact_int_fast_scalar_rejects_bad_argument_after_warmup() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function addOne(int $value): int { return $value + 1; }
for ($i = 0; $i < 100; $i++) { addOne($i); }
try { addOne("bad"); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_exact_int_fast_scalar_rejects_bad_return_after_warmup() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function maybeBad(int $value): int {
    if ($value < 1000) { return $value + 1; }
    return "bad";
}
for ($i = 0; $i < 100; $i++) { maybeBad($i); }
try { maybeBad(1000); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_hot_untyped_caller_rechecks_typed_scalar_callee() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function typedTarget(int $value): int { return $value + 1; }
function forward($value) { return typedTarget($value); }
for ($i = 0; $i < 100; $i++) { forward($i); }
try { forward(1.5); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_typed_scalar_method_rejects_bad_argument_after_warmup() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
class TypedCounter {
    function add(int $value): int { return $value + 1; }
}
$counter = new TypedCounter();
for ($i = 0; $i < 100; $i++) { $counter->add($i); }
try { $counter->add(1.5); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_fast_return_only_hint_rejects_bad_value_after_warmup() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function returnOnly($value): int { return $value; }
for ($i = 0; $i < 100; $i++) { returnOnly($i); }
try { returnOnly("bad"); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

// ── declare(strict_types=1) ──

#[test]
fn test_strict_types_float_rejects_int() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function f(float $x): void { echo $x; }
try { f(10); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_strict_types_float_accepts_float() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function f(float $x): void { echo $x; }
f(10.5);
"#), "10.5");
}

#[test]
fn test_no_strict_types_float_accepts_int() {
    assert_eq!(run_php(r#"<?php
function f(float $x): void { echo $x; }
f(10);
"#), "10");
}

#[test]
fn test_strict_types_int_still_works() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function f(int $x): void { echo $x; }
f(42);
"#), "42");
}

#[test]
fn test_strict_types_string_still_works() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function f(string $x): void { echo $x; }
f("hello");
"#), "hello");
}

#[test]
fn test_strict_types_return_type() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function f(): float { return 10; }
try { f(); } catch (TypeError $e) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_strict_types_0_allows_coercion() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=0);
function f(float $x): void { echo $x; }
f(10);
"#), "10");
}
