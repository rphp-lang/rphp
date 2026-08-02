/// Tests for parameter type hints
mod common;
use common::run_php;
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::instruction::{KnownScalarType, CALL_FLAG_EXACT_SCALAR_ARGS};
use rphp::vm::opcode::OpCode;

fn compile_types(source: &str) -> rphp::compiler::compile::CompileResult {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new().compile(&statements).unwrap()
}

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

// ── Declaration-derived scalar propagation ──

#[test]
fn test_exact_int_return_flows_into_caller_bytecode() {
    let result = compile_types(r#"<?php
function source(int $value): int { return $value % 97; }
function consume(int $value): int { return (source($value) % 13) ^ 3; }
"#);
    let consume = &result
        .functions
        .iter()
        .find(|(name, _)| name == "consume")
        .unwrap()
        .1
        .op_array;

    assert!(consume.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall
            && instruction.known_result_type() == KnownScalarType::Long
            && instruction._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
    }));
    assert!(consume
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::Mod_LongLong));
    assert!(consume
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::BitwiseXor_LongLong));
}

#[test]
fn test_exact_string_return_flows_through_concat_and_strlen() {
    let result = compile_types(r#"<?php
function source(string $value): string { return $value; }
function consume(string $value): int { return strlen(source($value) . "!"); }
"#);
    let consume = &result
        .functions
        .iter()
        .find(|(name, _)| name == "consume")
        .unwrap()
        .1
        .op_array;

    assert!(consume.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall
            && instruction.known_result_type() == KnownScalarType::String
    }));
    assert!(consume
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::Concat_StringString));
    assert!(consume
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::Strlen_String));
}

#[test]
fn test_mutable_typed_parameter_stays_on_guarded_strlen() {
    let result = compile_types(r#"<?php
function consume(int $value, bool $change): int {
    if ($change) { $value = "changed"; }
    return strlen($value);
}
"#);
    let consume = &result.functions[0].1.op_array;
    assert!(!consume
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::Strlen_String));
}

#[test]
fn test_unknown_argument_keeps_runtime_typed_call_guard() {
    let result = compile_types(r#"<?php
function target(int $value): int { return $value; }
function forward($value): int { return target($value); }
"#);
    let forward = &result
        .functions
        .iter()
        .find(|(name, _)| name == "forward")
        .unwrap()
        .1
        .op_array;
    assert!(forward.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall
            && instruction._pad & CALL_FLAG_EXACT_SCALAR_ARGS == 0
    }));
}

#[test]
fn test_propagated_int_and_string_operations_preserve_results() {
    assert_eq!(run_php(r#"<?php
function sourceInt(int $value): int { return $value % 97; }
function consumeInt(int $value): int { return (sourceInt($value) % 13) ^ 3; }
function sourceString(string $value): string { return $value; }
function consumeString(string $value): int { return strlen(sourceString($value) . "!"); }
echo consumeInt(12345);
echo ":";
echo consumeString("typed");
"#), "3:6");
}

#[test]
fn test_bad_declared_return_never_reaches_unguarded_consumer() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
function source(): int { return "bad"; }
function consume(): int { return source() % 7; }
try { consume(); } catch (TypeError $error) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_proven_long_modulo_handles_integer_minimum() {
    assert_eq!(run_php(r#"<?php
function remainder(int $left, int $right): int { return $left % $right; }
echo remainder(PHP_INT_MIN, -1);
"#), "0");
}

#[test]
fn test_proven_long_addition_still_validates_overflowed_return() {
    assert_eq!(run_php(r#"<?php
function add(int $left, int $right): int { return $left + $right; }
try { add(PHP_INT_MAX, 1); } catch (TypeError $error) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_method_return_contract_selects_one_dispatch_guard_and_scalar_consumers() {
    let result = compile_types(r#"<?php
class Source {
    function value(int $value): int {
        if (($value & 1) === 0) { return $value + 3; }
        return $value - 2;
    }
    function label(int $value): string {
        if (($value & 1) === 0) { return "even"; }
        return "odd";
    }
}
function consumeInt(Source $source, int $value): int {
    $result = $source->value($value);
    return ($result % 97) ^ 3;
}
function consumeString(Source $source, int $value): int {
    return strlen($source->label($value));
}
"#);
    let consume_int = &result
        .functions
        .iter()
        .find(|(name, _)| name == "consumeInt")
        .unwrap()
        .1
        .op_array;
    let guarded_init = consume_int
        .instructions
        .iter()
        .find(|instruction| instruction.opcode == OpCode::InitMethodCall)
        .unwrap();
    assert_eq!(
        guarded_init.method_return_guard_type(),
        KnownScalarType::Long
    );
    assert!(guarded_init.has_method_long_args_guard());
    assert!(consume_int
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::Mod_LongLong));
    assert!(consume_int.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall
            && instruction._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
    }));
    assert!(consume_int
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::BitwiseXor_LongLong));

    let consume_string = &result
        .functions
        .iter()
        .find(|(name, _)| name == "consumeString")
        .unwrap()
        .1
        .op_array;
    let string_init = consume_string
        .instructions
        .iter()
        .find(|instruction| instruction.opcode == OpCode::InitMethodCall)
        .unwrap();
    assert_eq!(
        string_init.method_return_guard_type(),
        KnownScalarType::String
    );
    assert!(consume_string
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::Strlen_String));

    assert_eq!(run_php(r#"<?php
class RuntimeSource {
    function value(int $value): int { return $value + 2; }
    function label(int $value): string { return "typed"; }
}
function runtimeConsume(RuntimeSource $source, int $value): int {
    return ($source->value($value) % 7) + strlen($source->label($value));
}
echo runtimeConsume(new RuntimeSource(), 5);
"#), "5");
}

#[test]
fn test_polymorphic_method_return_dispatch_accepts_compatible_override() {
    assert_eq!(run_php(r#"<?php
class IntegerSource { function value($value): int { return $value + 2; } }
class ShiftedSource extends IntegerSource { function value($value): int { return $value + 4; } }
function consume(IntegerSource $source, $value) { return $source->value($value) + 1; }
$integer = new IntegerSource();
$shifted = new ShiftedSource();
for ($i = 0; $i < 20; $i++) {
    consume($integer, $i);
    consume($shifted, $i);
}
echo consume($integer, 4);
echo ":";
echo consume($shifted, 4);
"#), "7:9");
}

#[test]
fn test_bad_typed_method_return_throws_before_guarded_consumer() {
    assert_eq!(run_php(r#"<?php
declare(strict_types=1);
class BadSource { function value(): int { return "bad"; } }
function consume(BadSource $source) { return $source->value() % 7; }
try { consume(new BadSource()); } catch (TypeError $error) { echo "caught"; }
"#), "caught");
}

#[test]
fn test_nullsafe_and_reference_receivers_do_not_use_method_return_guard() {
    let result = compile_types(r#"<?php
class Source { function label(): string { return "value"; } }
function nullable(?Source $source): int { return strlen($source?->label()); }
function referenced(Source &$source): int { return strlen($source->label()); }
"#);
    for name in ["nullable", "referenced"] {
        let function = &result
            .functions
            .iter()
            .find(|(candidate, _)| candidate == name)
            .unwrap()
            .1
            .op_array;
        assert!(function.instructions.iter().all(|instruction| {
            instruction.opcode != OpCode::InitMethodCall
                || instruction.method_return_guard_type() == KnownScalarType::Unknown
        }));
        assert!(function.instructions.iter().all(|instruction| {
            instruction.opcode != OpCode::Strlen_String
        }));
    }
}

#[test]
fn test_method_contract_flows_from_new_this_and_inheritance() {
    let result = compile_types(r#"<?php
class Source {
    function value(): int { return 42; }
    function fromThis(): int { return $this->value() % 5; }
}
class Child extends Source {}
class UntypedChild extends Source {
    function value() { return 42.5; }
}
function fromNew(): int {
    $source = new Source();
    return $source->value() % 5;
}
function fromInherited(Child $source): int {
    return $source->value() % 5;
}
function fromUntypedOverride(UntypedChild $source) {
    return $source->value() % 5;
}
"#);

    for name in ["fromNew", "fromInherited"] {
        let function = &result
            .functions
            .iter()
            .find(|(candidate, _)| candidate == name)
            .unwrap()
            .1
            .op_array;
        assert!(function
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::Mod_LongLong));
    }

    let source = result
        .class_defs
        .iter()
        .find(|class| class.name == "Source")
        .unwrap();
    let from_this = &source
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "fromThis")
        .unwrap()
        .4
        .op_array;
    assert!(from_this
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::Mod_LongLong));

    let untyped = &result
        .functions
        .iter()
        .find(|(name, _)| name == "fromUntypedOverride")
        .unwrap()
        .1
        .op_array;
    assert!(untyped.instructions.iter().all(|instruction| {
        instruction.opcode != OpCode::InitMethodCall
            || instruction.method_return_guard_type() == KnownScalarType::Unknown
    }));
    assert!(untyped
        .instructions
        .iter()
        .all(|instruction| instruction.opcode != OpCode::Mod_LongLong));
}
