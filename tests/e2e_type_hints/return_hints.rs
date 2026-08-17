// ── Return type hints ──

#[test]
fn test_return_type_int() {
    assert_eq!(
        run_php(
            r#"<?php
function add(int $a, int $b): int { return $a + $b; }
echo add(2, 3);
"#
        ),
        "5"
    );
}

#[test]
fn test_return_type_string() {
    assert_eq!(
        run_php(
            r#"<?php
function greet(string $name): string { return "Hello " . $name; }
echo greet("PHP");
"#
        ),
        "Hello PHP"
    );
}

#[test]
fn test_return_type_bool() {
    assert_eq!(
        run_php(
            r#"<?php
function isPositive(int $n): bool { return $n > 0; }
echo isPositive(5) ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_return_type_array() {
    assert_eq!(
        run_php(
            r#"<?php
function makeArr(): array { return [1, 2, 3]; }
echo count(makeArr());
"#
        ),
        "3"
    );
}

#[test]
fn test_static_return_type_uses_the_instance_and_static_called_class() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticReturnBase {
    public function copy(): static { return $this; }
    public function wrongInstance(): static { return new StaticReturnBase(); }
    public static function childFactory(): static { return new StaticReturnChild(); }
    public static function wrongFactory(): static { return new StaticReturnBase(); }
    public static function finallyFactory(): static {
        try { return new StaticReturnChild(); } finally { echo "finally:"; }
    }
    public static function fail(): static { throw new Exception("expected"); }
}
class StaticReturnChild extends StaticReturnBase {}
$value = new StaticReturnChild();
echo $value->copy() instanceof StaticReturnChild ? "instance:" : "bad:";
try { $value->wrongInstance(); } catch (TypeError $error) { echo "instance-error:"; }
echo StaticReturnChild::childFactory() instanceof StaticReturnChild ? "static:" : "bad:";
try { StaticReturnChild::wrongFactory(); } catch (TypeError $error) { echo "static-error"; }
echo ":";
echo StaticReturnChild::finallyFactory() instanceof StaticReturnChild ? "finally-return:" : "bad:";
try { StaticReturnChild::fail(); } catch (Exception $error) { echo "throw-cleanup"; }
"#
        ),
        "instance:instance-error:static:static-error:finally:finally-return:throw-cleanup"
    );
}

#[test]
fn test_static_call_sites_keep_the_shared_static_call_opcode() {
    let compiled = compile_types(
        r#"<?php
class StaticOpcodeBase {
    public static function ordinary() { return 1; }
    public static function late(): static { return new StaticOpcodeChild(); }
}
class StaticOpcodeChild extends StaticOpcodeBase {}
StaticOpcodeBase::ordinary();
StaticOpcodeChild::late();
ExternalStaticOpcode::unknown();
"#,
    );
    let opcodes = compiled
        .main
        .instructions
        .iter()
        .map(|instruction| instruction.opcode)
        .collect::<Vec<_>>();
    assert_eq!(
        opcodes
            .iter()
            .filter(|opcode| **opcode == OpCode::InitStaticCall)
            .count(),
        3
    );
}

#[test]
fn test_late_static_call_sites_use_a_separate_keyed_opcode() {
    let compiled = compile_types(
        r#"<?php
class LateStaticOpcodeBase {
    public static $number = 1;
    public static function value(): int { return 1; }
    public static function dispatch(): int { return static::value(); }
    public static function ordinary(): int { return self::value(); }
    public static function propertyDispatch(): int { return static::$number; }
    public static function ordinaryProperty(): int { return self::$number; }
    public static function propertyWrite(int $value): void { static::$number = $value; }
    public static function ordinaryPropertyWrite(int $value): void { self::$number = $value; }
    public function instanceDispatch(): int { return static::value(); }
}
"#,
    );
    let class = &compiled.class_defs[0];
    assert!(class.properties.is_empty());
    assert_eq!(class.static_properties.len(), 1);
    assert_eq!(class.static_properties[0].name, "number");
    let dispatch = class
        .methods
        .iter()
        .find(|(name, ..)| name == "dispatch")
        .map(|method| &method.4)
        .unwrap();
    let ordinary = class
        .methods
        .iter()
        .find(|(name, ..)| name == "ordinary")
        .map(|method| &method.4)
        .unwrap();
    let instance_dispatch = class
        .methods
        .iter()
        .find(|(name, ..)| name == "instanceDispatch")
        .map(|method| &method.4)
        .unwrap();
    let property_dispatch = class
        .methods
        .iter()
        .find(|(name, ..)| name == "propertyDispatch")
        .map(|method| &method.4)
        .unwrap();
    let ordinary_property = class
        .methods
        .iter()
        .find(|(name, ..)| name == "ordinaryProperty")
        .map(|method| &method.4)
        .unwrap();
    let property_write = class
        .methods
        .iter()
        .find(|(name, ..)| name == "propertyWrite")
        .map(|method| &method.4)
        .unwrap();
    let ordinary_property_write = class
        .methods
        .iter()
        .find(|(name, ..)| name == "ordinaryPropertyWrite")
        .map(|method| &method.4)
        .unwrap();

    assert!(
        dispatch
            .op_array
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::InitLateStaticCall)
    );
    assert!(
        ordinary
            .op_array
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::InitStaticCall)
    );
    assert!(
        ordinary
            .op_array
            .instructions
            .iter()
            .all(|instruction| instruction.opcode != OpCode::InitLateStaticCall)
    );
    assert!(dispatch.common.plan.needs_late_static_scope());
    assert!(property_dispatch
        .op_array
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::FetchLateStaticProp));
    assert!(ordinary_property
        .op_array
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::FetchStaticProp));
    assert!(property_write
        .op_array
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::AssignLateStaticProp));
    assert!(ordinary_property_write
        .op_array
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::AssignStaticProp));
    assert!(property_dispatch.common.plan.needs_late_static_scope());
    assert!(!ordinary_property.common.plan.needs_late_static_scope());
    assert!(!ordinary.common.plan.needs_late_static_scope());
    assert!(!instance_dispatch.common.plan.needs_late_static_scope());
}

#[test]
fn test_return_type_float() {
    assert_eq!(
        run_php(
            r#"<?php
function half(int $n): float { return $n / 2; }
echo half(7);
"#
        ),
        "3.5"
    );
}

#[test]
fn test_return_type_mismatch_throws() {
    assert_eq!(
        run_php(
            r#"<?php
function bad(): int { return "hello"; }
try {
    bad();
} catch (TypeError $e) {
    echo $e->getMessage();
}
"#
        ),
        "bad(): Return value must be of type int, string returned"
    );
}

#[test]
fn test_return_type_nullable_pass_null() {
    assert_eq!(
        run_php(
            r#"<?php
function maybe(): ?int { return null; }
echo maybe() === null ? "null" : "not null";
"#
        ),
        "null"
    );
}

#[test]
fn test_return_type_nullable_pass_value() {
    assert_eq!(
        run_php(
            r#"<?php
function maybe(): ?int { return 42; }
echo maybe();
"#
        ),
        "42"
    );
}

#[test]
fn test_return_type_nullable_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function maybe(): ?int { return "oops"; }
try { maybe(); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

// ── Void return type ──

#[test]
fn test_void_bare_return() {
    assert_eq!(
        run_php(
            r#"<?php
function doStuff(): void { echo "done"; return; }
doStuff();
"#
        ),
        "done"
    );
}

#[test]
fn test_void_implicit_return() {
    assert_eq!(
        run_php(
            r#"<?php
function doStuff(): void { echo "done"; }
doStuff();
"#
        ),
        "done"
    );
}

#[test]
fn test_void_return_value_error() {
    let error = run_php_expect_error("<?php function bad(): void { return 42; }");
    assert!(
        format!("{error:?}").contains("A void function must not return a value"),
        "{error:?}"
    );
}

// ── Mixed return type ──

#[test]
fn test_mixed_return_int() {
    assert_eq!(
        run_php(
            r#"<?php
function f(): mixed { return 42; }
echo f();
"#
        ),
        "42"
    );
}

#[test]
fn test_mixed_return_string() {
    assert_eq!(
        run_php(
            r#"<?php
function f(): mixed { return "hello"; }
echo f();
"#
        ),
        "hello"
    );
}

#[test]
fn test_mixed_return_null() {
    assert_eq!(
        run_php(
            r#"<?php
function f(): mixed { return null; }
echo f() === null ? "null" : "other";
"#
        ),
        "null"
    );
}

#[test]
fn declared_return_types_reject_missing_values_but_accept_explicit_null() {
    assert_eq!(
        run_php(
            r#"<?php
function missingMixed(): mixed {}
function missingNullable(): ?int {}
function explicitMixed(): mixed { return null; }
function explicitNullable(): ?int { return null; }
function implicitUntyped() {}

foreach (["missingMixed", "missingNullable"] as $function) {
    try { $function(); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
var_dump(explicitMixed(), explicitNullable(), implicitUntyped());
"#
        ),
        "missingMixed(): Return value must be of type mixed, none returned\nmissingNullable(): Return value must be of type ?int, none returned\nNULL\nNULL\nNULL\n"
    );
}

#[test]
fn type_errors_use_canonical_declared_and_concrete_runtime_names() {
    assert_eq!(
        run_php(
            r#"<?php
class Expected {}
function acceptExpected(?Expected $value): void {}
function acceptIterable(?iterable $value): void {}
function returnExpected(): ?Expected { return new stdClass(); }

try { acceptExpected(new stdClass()); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { acceptIterable(1); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { returnExpected(); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#
        ),
        "acceptExpected(): Argument #1 ($value) must be of type ?Expected, stdClass given, called in <main> on line 7\nacceptIterable(): Argument #1 ($value) must be of type Traversable|array|null, int given, called in <main> on line 9\nreturnExpected(): Return value must be of type ?Expected, stdClass returned\n"
    );
}

#[test]
fn bare_returns_obey_declared_return_contracts_at_compile_time() {
    for (source, expected) in [
        (
            "<?php function nullable(): ?int { return; }",
            "A function with return type must return a value (did you mean \"return null;\" instead of \"return;\"?)",
        ),
        (
            "<?php function scalar(): int { return; }",
            "A function with return type must return a value",
        ),
        (
            "<?php function bottom(): never { return; }",
            "A never-returning function must not return",
        ),
        (
            "<?php class Bottom { public function stop(): never { return; } }",
            "A never-returning method must not return",
        ),
    ] {
        let error = run_php_expect_error(source);
        let rphp::vm::execute::VmError::Fatal(message) = error else {
            panic!("unexpected compile error: {error:?}");
        };
        assert!(
            message.contains(expected),
            "unexpected compile error: {message}"
        );
    }

    assert_eq!(
        run_php("<?php function nothing(): void { return; } nothing(); echo 'ok';"),
        "ok"
    );
    assert_eq!(
        run_php(
            "<?php function numbers(): Iterator { yield 1; return; } foreach (numbers() as $value) { echo $value; }"
        ),
        "1"
    );
}

#[test]
fn never_and_void_reject_value_returns_and_parameter_positions_during_compilation() {
    for (source, expected) in [
        (
            "<?php function stop(): never { return throw new Exception('no'); }",
            "A never-returning function must not return",
        ),
        (
            "<?php function nothing(): void { return null; }",
            "A void function must not return a value (did you mean \"return;\" instead of \"return null;\"?)",
        ),
        (
            "<?php class C { function nothing(): void { return 1; } }",
            "A void method must not return a value",
        ),
        (
            "<?php function consume(never $value) {}",
            "never cannot be used as a parameter type",
        ),
        (
            "<?php function consume(void $value) {}",
            "void cannot be used as a parameter type",
        ),
        (
            "<?php function invalidGenerator(): stdClass|array { yield 1; }",
            "Generator return type must be a supertype of Generator, stdClass|array given",
        ),
        (
            "<?php class C { function invalidGenerator(): int { yield 1; } }",
            "Generator return type must be a supertype of Generator, int given",
        ),
        (
            "<?php $invalid = function(): string { yield 1; };",
            "Generator return type must be a supertype of Generator, string given",
        ),
    ] {
        let error = run_php_expect_error(source);
        let rphp::vm::execute::VmError::Fatal(message) = error else {
            panic!("unexpected compile error: {error:?}");
        };
        assert!(message.contains(expected), "unexpected compile error: {message}");
    }

    assert_eq!(
        run_php(
            "<?php function validGenerator(): object|callable { yield 1; } echo validGenerator() instanceof Generator ? 'ok' : 'bad';"
        ),
        "ok"
    );
}

// ── Never return type ──

#[test]
fn test_never_throws_ok() {
    assert_eq!(
        run_php(
            r#"<?php
function fail(): never { throw new Exception("bye"); }
try { fail(); } catch (Exception $e) { echo $e->getMessage(); }
"#
        ),
        "bye"
    );
}

#[test]
fn test_never_return_error() {
    let error = run_php_expect_error("<?php function bad(): never { return 42; }");
    assert!(
        format!("{error:?}").contains("A never-returning function must not return"),
        "{error:?}"
    );
}

// ── Union types ──

#[test]
fn test_union_return_int_ok() {
    assert_eq!(
        run_php(
            r#"<?php
function f(): int|string { return 42; }
echo f();
"#
        ),
        "42"
    );
}

#[test]
fn test_union_return_string_ok() {
    assert_eq!(
        run_php(
            r#"<?php
function f(): int|string { return "hello"; }
echo f();
"#
        ),
        "hello"
    );
}

#[test]
fn test_union_return_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function f(): int|string { return [1,2]; }
try { f(); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn literal_false_in_callable_union_is_not_namespaced() {
    assert_eq!(
        run_php(
            r#"<?php
namespace Feature;

function choose(bool $enabled): callable|false {
    if (!$enabled) {
        return false;
    }
    return fn() => 'called';
}

function passThrough(callable|false $value): callable|false {
    return $value;
}

echo passThrough(choose(false)) === false ? 'false' : 'bad';
echo '|';
$callback = passThrough(choose(true));
echo $callback();
"#,
        ),
        "false|called"
    );
}

#[test]
fn test_union_param_types() {
    assert_eq!(
        run_php(
            r#"<?php
function show(int|string $x): void { echo $x; }
show(42);
echo " ";
show("hi");
"#
        ),
        "42 hi"
    );
}

#[test]
fn test_union_param_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function show(int|string $x): void { echo $x; }
try { show([1]); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_union_three_types() {
    assert_eq!(
        run_php(
            r#"<?php
function f(int|string|bool $x): void { echo $x; }
f(42);
f("hi");
f(true);
"#
        ),
        "42hi1"
    );
}

// ── Class return type hints ──

#[test]
fn test_return_type_class() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo { public $x; public function __construct($x) { $this->x = $x; } }
function makeFoo(): Foo { return new Foo(42); }
$f = makeFoo();
echo $f->x;
"#
        ),
        "42"
    );
}

#[test]
fn test_return_type_class_fail() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
function makeFoo(): Foo { return "not an object"; }
try { makeFoo(); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

// ── Method return types ──

#[test]
fn test_method_return_type() {
    assert_eq!(
        run_php(
            r#"<?php
class Calc {
    public function add(int $a, int $b): int { return $a + $b; }
}
$c = new Calc();
echo $c->add(3, 4);
"#
        ),
        "7"
    );
}

#[test]
fn test_method_return_type_fail() {
    assert_eq!(
        run_php(
            r#"<?php
class Calc {
    public function bad(): int { return "nope"; }
}
$c = new Calc();
try { $c->bad(); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

// ── Closure & arrow function return types ──

#[test]
fn test_closure_return_type() {
    assert_eq!(
        run_php(
            r#"<?php
$f = function(int $x): int { return $x * 2; };
echo $f(5);
"#
        ),
        "10"
    );
}

#[test]
fn test_closure_return_type_fail() {
    assert_eq!(
        run_php(
            r#"<?php
$f = function(): int { return "bad"; };
try { $f(); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_arrow_fn_return_type() {
    assert_eq!(
        run_php(
            r#"<?php
$f = fn(int $x): int => $x * 3;
echo $f(4);
"#
        ),
        "12"
    );
}

#[test]
fn test_arrow_fn_return_type_fail() {
    assert_eq!(
        run_php(
            r#"<?php
$f = fn(): int => "bad";
try { $f(); } catch (TypeError $e) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn magic_method_return_types_accept_php_covariant_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
class AcceptedMagicReturns {
    public function __clone(): void {}
    public function __isset($name): true { return true; }
    public function __toString(): never { throw new Exception(); }
    public function __debugInfo(): null { return null; }
    public function __serialize(): array { return []; }
    public function __unserialize(array $data): void {}
    public static function __set_state($properties): self { return new self(); }
}
echo 'ok';
"#,
        ),
        "ok"
    );
}

#[test]
fn magic_method_return_types_reject_incompatible_declarations() {
    for (source, expected) in [
        (
            "<?php class BadConstructor { function __construct(): void {} }",
            "Method BadConstructor::__construct() cannot declare a return type",
        ),
        (
            "<?php class BadClone { function __clone(): int {} }",
            "BadClone::__clone(): Return type must be void when declared",
        ),
        (
            "<?php class BadIsset { function __isset($name): object|bool {} }",
            "BadIsset::__isset(): Return type must be bool when declared",
        ),
        (
            "<?php class BadDebug { function __debugInfo(): bool {} }",
            "BadDebug::__debugInfo(): Return type must be ?array when declared",
        ),
        (
            "<?php class BadState { static function __set_state($properties): bool {} }",
            "BadState::__set_state(): Return type must be object when declared",
        ),
        (
            "<?php interface BadStringable { public function __toString(): bool; }",
            "BadStringable::__toString(): Return type must be string when declared",
        ),
        (
            "<?php trait BadSerialization { public function __serialize(): object {} }",
            "BadSerialization::__serialize(): Return type must be array when declared",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert!(error.to_string().contains(expected), "unexpected error: {error}");
    }
}

#[test]
fn enums_allow_invocation_magic_but_reject_state_and_lifecycle_magic() {
    assert_eq!(
        run_php(
            "<?php enum CallableEnum { case Value; public function __invoke() { return 1; } public function __call($name, $arguments) {} public static function __callStatic($name, $arguments) {} } echo (CallableEnum::Value)();",
        ),
        "1"
    );
    for method in ["__construct", "__clone", "__get", "__serialize", "__set_state"] {
        let static_prefix = if method == "__set_state" { "static " } else { "" };
        let parameters = match method {
            "__get" => "$name",
            "__set_state" => "$properties",
            _ => "",
        };
        let source = format!(
            "<?php enum ForbiddenMagic {{ case Value; public {static_prefix}function {method}({parameters}) {{}} }}"
        );
        let error = run_php_expect_error(&source);
        assert!(
            error
                .to_string()
                .contains(&format!("Enum ForbiddenMagic cannot include magic method {method}")),
            "unexpected error: {error}"
        );
    }
}
