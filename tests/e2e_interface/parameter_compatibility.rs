// ── P1 repro: Interface arity with optional params on interface side ──

#[test]
fn test_interface_optional_param_impl_makes_required_rejected() {
    // Interface: foo($x = 1) — $x is optional.
    // Class: foo($x) — $x is required.
    // A caller following the interface contract may call foo() with 0 args,
    // but the implementation requires 1. Must be rejected.
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo($x = 1);
}
class C implements I {
    public function foo($x) {}
}
echo "accepted";
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic: impl requires param that interface makes optional"
    );
}

#[test]
fn test_interface_mixed_optional_impl_makes_required_rejected() {
    // Interface: foo($x, $y = 1) — $y is optional.
    // Class: foo($x, $y) — $y is required.
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo($x, $y = 1);
}
class C implements I {
    public function foo($x, $y) {}
}
echo "accepted";
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic: impl requires param that interface makes optional"
    );
}

#[test]
fn test_interface_optional_param_impl_also_optional_ok() {
    // Interface: foo($x = 1) — $x is optional.
    // Class: foo($x = 2) — $x is also optional. Valid.
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo($x = 1);
}
class C implements I {
    public function foo($x = 2) { echo $x; }
}
$c = new C();
$c->foo();
"#
        ),
        "2"
    );
}

// ── P2 repro: Interface parser rejects non-public methods ──

#[test]
fn test_interface_private_method_parse_error() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    private function foo();
}
echo "made";
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected parse error from private method in interface"
    );
}

#[test]
fn test_interface_protected_method_parse_error() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    protected function foo();
}
echo "made";
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected parse error from protected method in interface"
    );
}

// ── Interface parameter type compatibility (contravariance) ──

#[test]
fn test_interface_param_type_exact_match_ok() {
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(int $x);
}
class C implements I {
    public function foo(int $x) { echo $x; }
}
$c = new C();
$c->foo(42);
"#
        ),
        "42"
    );
}

#[test]
fn test_interface_param_narrowing_rejected() {
    // Interface accepts A, implementation narrows to B extends A → rejected (fewer valid args)
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
class A {}
class B extends A {}
interface I {
    public function foo(A $x);
}
class C implements I {
    public function foo(B $x) {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from narrowing param type in interface implementation"
    );
}

#[test]
fn test_interface_param_widening_ok() {
    // Interface accepts B, implementation widens to A (parent) → ok (contravariance)
    assert_eq!(
        run_php(
            r#"<?php
class A {}
class B extends A {}
interface I {
    public function foo(B $x);
}
class C implements I {
    public function foo(A $x) { echo "ok"; }
}
$c = new C();
$c->foo(new B());
"#
        ),
        "ok"
    );
}

#[test]
fn test_interface_param_no_type_in_impl_ok() {
    // Interface has type hint, implementation has none → always compatible
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(int $x);
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
fn test_interface_param_scalar_mismatch_rejected() {
    // Interface declares string, implementation declares int → incompatible
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo(string $x);
}
class C implements I {
    public function foo(int $x) {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from scalar type mismatch in interface param"
    );
}

#[test]
fn test_interface_param_nullable_widening_ok() {
    // Interface: int, implementation: ?int → impl accepts more (null too), ok
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo(int $x);
}
class C implements I {
    public function foo(?int $x) { echo $x === null ? "null" : $x; }
}
$c = new C();
$c->foo(5);
"#
        ),
        "5"
    );
}

#[test]
fn test_interface_param_nullable_narrowing_rejected() {
    // Interface: ?int, implementation: int → impl rejects null, incompatible
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo(?int $x);
}
class C implements I {
    public function foo(int $x) {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from nullable narrowing in interface param"
    );
}

#[test]
fn test_interface_multiple_params_second_mismatch() {
    // First param matches, second param has type mismatch
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo(int $a, string $b);
}
class C implements I {
    public function foo(int $a, int $b) {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from second param type mismatch"
    );
}
