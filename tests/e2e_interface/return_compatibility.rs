// ── Interface return type covariance edge cases ──

#[test]
fn test_interface_return_covariant_class_ok() {
    // Interface returns A, implementation returns B extends A → ok (covariance)
    assert_eq!(
        run_php(
            r#"<?php
class A {}
class B extends A {}
interface I {
    public function make(): A;
}
class C implements I {
    public function make(): B { return new B(); }
}
$c = new C();
echo $c->make() instanceof A ? "ok" : "fail";
"#
        ),
        "ok"
    );
}

#[test]
fn test_interface_return_static_from_trait_is_covariant_with_object() {
    assert_eq!(
        run_php(
            r#"<?php
interface Initializable {
    public function initialize(): object;
}
trait LazyInitializer {
    public function initialize(): static { return $this; }
}
class LazyService implements Initializable {
    use LazyInitializer;
}
echo (new LazyService())->initialize() instanceof LazyService ? "ok" : "fail";
"#
        ),
        "ok"
    );
}

#[test]
fn test_interface_iterable_return_accepts_array_covariance() {
    assert_eq!(
        run_php(
            r#"<?php
interface IterableResult { public function values(): iterable; }
class ArrayResult implements IterableResult {
    public function values(): array { return [1, 2, 3]; }
}
echo count((new ArrayResult())->values());
"#,
        ),
        "3"
    );
}

#[test]
fn test_interface_array_return_rejects_iterable_widening() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface ArrayResult { public function values(): array; }
class IterableResult implements ArrayResult {
    public function values(): iterable { return []; }
}
"#,
        )
    });
    assert!(result.is_err(), "Expected iterable to widen an array return");
}

#[test]
fn test_interface_return_widening_rejected() {
    // Interface returns B, implementation returns A (parent) → rejected (too wide)
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
class A {}
class B extends A {}
interface I {
    public function make(): B;
}
class C implements I {
    public function make(): A { return new A(); }
}
"#,
        )
    });
    assert!(result.is_err(), "Expected panic from widening return type");
}

#[test]
fn test_interface_return_nullable_narrowing_ok() {
    // Interface: ?int, implementation: int → ok (narrowing return is fine)
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function get(): ?int;
}
class C implements I {
    public function get(): int { return 42; }
}
$c = new C();
echo $c->get();
"#
        ),
        "42"
    );
}

#[test]
fn test_interface_return_nullable_widening_rejected() {
    // Interface: int, implementation: ?int → rejected (might return null)
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function get(): int;
}
class C implements I {
    public function get(): ?int { return null; }
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from widening return to nullable"
    );
}

// ── Interface param union widening (contravariance) ──

#[test]
fn test_interface_param_union_widening_ok() {
    // Interface: int, implementation: int|float → impl accepts more, ok
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(int $x);
}
class C implements I {
    public function foo(int|float $x) { echo $x; }
}
$c = new C();
$c->foo(42);
"#
        ),
        "42"
    );
}

#[test]
fn test_interface_param_union_narrowing_rejected() {
    // Interface: int|string, implementation: int → impl accepts fewer, rejected
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo(int|string $x);
}
class C implements I {
    public function foo(int $x) {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from union narrowing in interface param"
    );
}

// ── Interface return type: mixed requires explicit declaration ──

#[test]
fn test_interface_return_mixed_no_type_rejected() {
    // Interface: mixed, implementation: no type → rejected (must be explicit)
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo(): mixed;
}
class C implements I {
    public function foo() {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic: impl must declare return type when interface declares mixed"
    );
}

#[test]
fn test_interface_return_mixed_with_explicit_mixed_ok() {
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(): mixed;
}
class C implements I {
    public function foo(): mixed { return 42; }
}
$c = new C();
echo $c->foo();
"#
        ),
        "42"
    );
}

#[test]
fn test_interface_return_mixed_with_int_ok() {
    // mixed in interface, int in impl → ok (any explicit type is compatible)
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(): mixed;
}
class C implements I {
    public function foo(): int { return 42; }
}
$c = new C();
echo $c->foo();
"#
        ),
        "42"
    );
}

// ── Interface return type: union narrowing (covariance) ──

#[test]
fn test_interface_return_union_narrowing_ok() {
    // Interface: int|float, implementation: int → ok (covariant narrowing)
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(): int|float;
}
class C implements I {
    public function foo(): int { return 42; }
}
$c = new C();
echo $c->foo();
"#
        ),
        "42"
    );
}

#[test]
fn test_interface_return_union_widening_rejected() {
    // Interface: int, implementation: int|string → rejected (might return string)
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo(): int;
}
class C implements I {
    public function foo(): int|string { return "oops"; }
}
"#,
        )
    });
    assert!(result.is_err(), "Expected panic from widening return union");
}

// ── Interface: untyped param must not be narrowed ──

#[test]
fn test_interface_untyped_param_narrowed_rejected() {
    // Interface: foo($x), implementation: foo(int $x) → rejected
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo($x);
}
class C implements I {
    public function foo(int $x) {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic: impl narrows untyped interface param"
    );
}

#[test]
fn test_interface_untyped_param_impl_untyped_ok() {
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo($x);
}
class C implements I {
    public function foo($x) { echo $x; }
}
$c = new C();
$c->foo(42);
"#
        ),
        "42"
    );
}

#[test]
fn test_interface_untyped_param_impl_mixed_ok() {
    // mixed is equivalent to untyped — should be ok
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo($x);
}
class C implements I {
    public function foo(mixed $x) { echo $x; }
}
$c = new C();
$c->foo("hi");
"#
        ),
        "hi"
    );
}

// ── Interface: never return type covariance ──

#[test]
fn test_interface_return_never_covariant_ok() {
    // never is bottom type — covariant with any return type
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(): int;
}
class C implements I {
    public function foo(): never { throw new \Exception("bye"); }
}
$c = new C();
try { $c->foo(); } catch (\Exception $e) { echo $e->getMessage(); }
"#
        ),
        "bye"
    );
}

#[test]
fn test_interface_return_never_with_mixed_ok() {
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(): mixed;
}
class C implements I {
    public function foo(): never { throw new \Exception("stop"); }
}
$c = new C();
try { $c->foo(); } catch (\Exception $e) { echo $e->getMessage(); }
"#
        ),
        "stop"
    );
}

// ── Interface: mixed return must reject void ──

#[test]
fn test_interface_return_mixed_rejects_void() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo(): mixed;
}
class C implements I {
    public function foo(): void {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic: void is not compatible with mixed return"
    );
}
