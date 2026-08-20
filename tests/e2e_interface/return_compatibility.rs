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
fn final_class_may_close_late_static_return_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
interface FinalStaticContract {
    public function direct(): static;
    public function compound(): static|string;
    public function nullable(): static|null;
}
abstract class FinalStaticBase {
    abstract public function inherited(): static;
}
trait FinalStaticRequirement {
    abstract public function composed(): static;
}
final class FinalStaticResult extends FinalStaticBase implements FinalStaticContract {
    use FinalStaticRequirement;
    public function direct(): self { return $this; }
    public function compound(): self|string { return $this; }
    public function nullable(): ?self { return $this; }
    public function inherited(): FinalStaticResult { return $this; }
    public function composed(): self { return $this; }
}
$result = new FinalStaticResult();
echo (int) ($result->direct() === $result);
echo (int) ($result->compound() === $result);
echo (int) ($result->nullable() === $result);
echo (int) ($result->inherited() === $result);
echo (int) ($result->composed() === $result);
"#,
        ),
        "11111"
    );
}

#[test]
fn non_final_class_cannot_close_a_late_static_return_contract() {
    let error = run_php_expect_error(
        r#"<?php
interface OpenStaticContract {
    public function make(): static;
}
class OpenStaticResult implements OpenStaticContract {
    public function make(): self { return $this; }
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of OpenStaticResult::make(): OpenStaticResult must be compatible with OpenStaticContract::make(): static\")"
    );
}

#[test]
fn final_static_union_still_rejects_an_unrelated_branch() {
    let error = run_php_expect_error(
        r#"<?php
interface ClosedUnionContract {
    public function make(): static|bool;
}
final class ClosedUnionResult implements ClosedUnionContract {
    public function make(): self|array { return []; }
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of ClosedUnionResult::make(): ClosedUnionResult|array must be compatible with ClosedUnionContract::make(): static|bool\")"
    );
}

#[test]
fn visibility_error_precedes_a_signature_mismatch() {
    let error = run_php_expect_error(
        r#"<?php
interface VisibilityContract {
    public function run(int $value);
}
class VisibilityChild implements VisibilityContract {
    protected function run(string $value) {}
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Access level to VisibilityChild::run() must be public (as in class VisibilityContract)\")"
    );
}

#[test]
fn protected_visibility_diagnostic_names_the_weaker_boundary() {
    let error = run_php_expect_error(
        r#"<?php
abstract class ProtectedContract {
    abstract protected function run();
}
class PrivateChild extends ProtectedContract {
    private function run() {}
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Access level to PrivateChild::run() must be protected (as in class ProtectedContract) or weaker\")"
    );
}

#[test]
fn trait_method_diagnostics_use_the_composing_class() {
    let visibility_error = run_php_expect_error(
        r#"<?php
interface PublicTraitContract {
    public function run();
}
trait ProtectedTraitImplementation {
    protected function run() {}
}
class ProtectedTraitConsumer implements PublicTraitContract {
    use ProtectedTraitImplementation;
}
"#,
    );
    assert_eq!(
        format!("{visibility_error:?}"),
        "Fatal(\"Access level to ProtectedTraitConsumer::run() must be public (as in class PublicTraitContract)\")"
    );

    let signature_error = run_php_expect_error(
        r#"<?php
interface ArityTraitContract {
    public function run($value);
}
trait ArityTraitImplementation {
    public function run() {}
}
class ArityTraitConsumer implements ArityTraitContract {
    use ArityTraitImplementation;
}
"#,
    );
    assert_eq!(
        format!("{signature_error:?}"),
        "Fatal(\"Declaration of ArityTraitConsumer::run() must be compatible with ArityTraitContract::run($value)\")"
    );
}

#[test]
fn method_diagnostics_render_scalar_and_array_defaults() {
    let error = run_php_expect_error(
        r#"<?php
class DefaultContract {
    public function run(?array $items = NuLl, $label = "abcdefghijklmnop", $shape = [1]) {}
}
class DefaultImplementation extends DefaultContract {
    public function run(array $items = [], $label = "short", $shape = []) {}
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of DefaultImplementation::run(array $items = [], $label = 'short', $shape = []) must be compatible with DefaultContract::run(?array $items = null, $label = 'abcdefghij...', $shape = [...])\")"
    );
}

#[test]
fn method_diagnostics_omit_defaults_before_required_parameters() {
    let error = run_php_expect_error(
        r#"<?php
class RequiredContract {
    public function run(?array $items = null, $required) {}
}
class RequiredImplementation extends RequiredContract {
    public function run(?array $items) {}
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of RequiredImplementation::run(?array $items) must be compatible with RequiredContract::run(?array $items, $required)\")"
    );
}

#[test]
fn method_diagnostics_resolve_default_constant_names() {
    let error = run_php_expect_error(
        r#"<?php
namespace DiagnosticSource {
    const VALUE = 1;
    class Token { public const VALUE = 2; }
}
namespace DiagnosticConsumer {
    use const DiagnosticSource\VALUE;
    use DiagnosticSource\Token as TokenAlias;
    class DefaultContract {
        public function run($value = VALUE, $token = TokenAlias::VALUE) {}
    }
    class DefaultImplementation extends DefaultContract {
        public function run() {}
    }
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of DiagnosticConsumer\\\\DefaultImplementation::run() must be compatible with DiagnosticConsumer\\\\DefaultContract::run($value = DiagnosticSource\\\\VALUE, $token = DiagnosticSource\\\\Token::VALUE)\")"
    );
}

#[test]
fn dynamic_class_constant_defaults_use_expression_placeholder() {
    let error = run_php_expect_error(
        r#"<?php
class DynamicDefaultContract {
    public function run(int $value) {}
}
class DynamicDefaultImplementation extends DynamicDefaultContract {
    public function run(string $value = MissingOwner::{MISSING_NAME}) {}
}
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of DynamicDefaultImplementation::run(string $value = <expression>) must be compatible with DynamicDefaultContract::run(int $value)\")"
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
