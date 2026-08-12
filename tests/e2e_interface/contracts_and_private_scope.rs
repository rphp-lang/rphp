// ── P1: Interface contract enforcement ──

#[test]
fn test_missing_interface_method_rejected() {
    // Class C implements I but does NOT provide foo() → must error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo();
}
class C implements I {}
$c = new C();
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from missing interface method"
    );
}

#[test]
fn test_interface_method_provided_ok() {
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo();
}
class C implements I {
    public function foo() { echo "ok"; }
}
$c = new C();
$c->foo();
"#
        ),
        "ok"
    );
}

// ── P1: Visibility uses lexical class scope ──

#[test]
fn test_private_not_accessible_from_child() {
    // PHP: private methods are NOT accessible from child classes
    let err = run_php_expect_error(
        r#"<?php
class A {
    private function secret() { echo "A"; }
}
class B extends A {
    public function probe() { $this->secret(); }
}
$b = new B();
$b->probe();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("private"), "got: {}", msg);
        }
        other => panic!(
            "Expected Fatal for private access from child, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_static_method_visibility_with_scope() {
    // Static method call from within the same class should work
    assert_eq!(
        run_php(
            r#"<?php
class A {
    private static function secret() { echo "A"; }
    public static function reveal() { A::secret(); }
}
A::reveal();
"#
        ),
        "A"
    );
}

// ── P2: Abstract class not instantiable ──

#[test]
fn test_abstract_class_not_instantiable() {
    let err = run_php_expect_error(
        r#"<?php
abstract class A {}
$a = new A();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Cannot instantiate abstract class"),
                "got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_abstract_class_child_instantiable() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class Shape {
    public function name() { echo "shape"; }
}
class Circle extends Shape {}
$c = new Circle();
$c->name();
"#
        ),
        "shape"
    );
}

#[test]
fn test_abstract_method_implemented_by_concrete_child() {
    assert_eq!(
        run_php(
            r#"<?php
abstract class Shape {
    abstract public function area(): int;
}
class Square extends Shape {
    public function area(): int { return 16; }
}
$shape = new Square();
echo $shape->area();
"#,
        ),
        "16"
    );
}

#[test]
fn test_abstract_method_required_for_concrete_child() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
abstract class Shape {
    abstract public function area(): int;
}
class MissingArea extends Shape {}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected concrete child without abstract implementation to fail"
    );
}

#[test]
fn test_abstract_trait_method_contract() {
    assert_eq!(
        run_php(
            r#"<?php
trait NeedsName {
    abstract public function name(): string;
}
class NamedThing {
    use NeedsName;
    public function name(): string { return "thing"; }
}
echo (new NamedThing())->name();
"#,
        ),
        "thing"
    );

    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait NeedsName {
    abstract public function name(): string;
}
class MissingName { use NeedsName; }
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected an unsatisfied abstract trait method to fail"
    );
}

#[test]
fn test_abstract_method_contract_checks_signature() {
    let incompatible_parameters = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
abstract class Formatter {
    abstract public function format(int $value, string $suffix = ""): string;
}
class NarrowFormatter extends Formatter {
    public function format(string $value): string { return $value; }
}
"#,
        )
    });
    assert!(
        incompatible_parameters.is_err(),
        "Expected an incompatible abstract parameter contract to fail"
    );

    let incompatible_return = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
abstract class Factory {
    abstract public function create(): object;
}
class ScalarFactory extends Factory {
    public function create(): int { return 1; }
}
"#,
        )
    });
    assert!(
        incompatible_return.is_err(),
        "Expected an incompatible abstract return contract to fail"
    );
}

#[test]
fn test_abstract_method_contract_checks_visibility_staticness_and_references() {
    let visibility = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
abstract class Base {
    abstract public function run(): void;
}
class Hidden extends Base {
    protected function run(): void {}
}
"#,
        )
    });
    assert!(visibility.is_err(), "Expected visibility narrowing to fail");

    let staticness = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
abstract class Base {
    abstract public static function run(): void;
}
class InstanceRun extends Base {
    public function run(): void {}
}
"#,
        )
    });
    assert!(staticness.is_err(), "Expected staticness mismatch to fail");

    let references = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
abstract class Mutator {
    abstract public function mutate(&$value): void;
}
class ByValueMutator extends Mutator {
    public function mutate($value): void {}
}
"#,
        )
    });
    assert!(references.is_err(), "Expected reference mode mismatch to fail");
}

#[test]
fn test_abstract_trait_method_retains_visibility_compatibility_exception() {
    assert_eq!(
        run_php(
            r#"<?php
trait RequiresWork {
    abstract public function work(): void;
}
class Worker {
    use RequiresWork;
    private function work(): void {}
}
echo "ok";
"#,
        ),
        "ok"
    );
}

// ── P1 repro: Private method early binding (parent vs child) ──

#[test]
fn test_private_method_early_binding() {
    // A::callA() calls $this->who() — must dispatch to A::who(), not B::who()
    assert_eq!(
        run_php(
            r#"<?php
class A {
    private function who() { echo "A"; }
    public function callA() { $this->who(); }
}
class B extends A {
    private function who() { echo "B"; }
}
$b = new B();
$b->callA();
"#
        ),
        "A"
    );
}

// ── P1 repro: Private property separate slots ──

#[test]
fn test_private_property_separate_slots() {
    // Parent and child both have private $x — must be separate storage
    assert_eq!(
        run_php(
            r#"<?php
class A {
    private $x = 1;
    public function a() { echo $this->x; }
}
class B extends A {
    private $x = 2;
    public function b() { echo $this->x; }
}
$o = new B();
$o->a();
$o->b();
"#
        ),
        "12"
    );
}

// ── P1 repro: Interface validation checks visibility ──

#[test]
fn test_interface_method_must_be_public() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo();
}
class C implements I {
    private function foo() {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from private implementation of interface method"
    );
}

// ── P2 repro: Inherited interface obligations from abstract parent ──

#[test]
fn test_inherited_interface_from_abstract_parent() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo();
}
abstract class A implements I {}
class C extends A {}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from unimplemented interface method inherited via abstract parent"
    );
}

#[test]
fn test_inherited_interface_satisfied_by_child() {
    // Child C implements foo() — satisfying the interface from abstract parent A
    assert_eq!(
        run_php(
            r#"<?php
interface I {
    public function foo();
}
abstract class A implements I {}
class C extends A {
    public function foo() { echo "ok"; }
}
$c = new C();
$c->foo();
"#
        ),
        "ok"
    );
}

// ── P1 repro: Private method early binding must not leak across unrelated objects ──

#[test]
fn test_private_method_no_leak_to_unrelated_object() {
    let err = run_php_expect_error(
        r#"<?php
class A {
    private function who() { echo "A"; }
    public function callOther($o) { $o->who(); }
}
class B {
    private function who() { echo "B"; }
}
$a = new A();
$b = new B();
$a->callOther($b);
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("private"),
                "Expected private error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_private_method_early_binding_still_works_for_child() {
    assert_eq!(
        run_php(
            r#"<?php
class A {
    private function who() { echo "A"; }
    public function callA() { $this->who(); }
}
class B extends A {
    private function who() { echo "B"; }
}
$b = new B();
$b->callA();
"#
        ),
        "A"
    );
}

// ── P1 repro: Private property must not leak to unrelated objects ──

#[test]
fn test_private_property_no_read_leak_to_unrelated_object() {
    let err = run_php_expect_error(
        r#"<?php
class A {
    private $x = 1;
    public function readOther($o) { echo $o->x; }
}
class B {
    private $x = 2;
}
$a = new A();
$b = new B();
$a->readOther($b);
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("private"),
                "Expected private error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_private_property_no_write_leak_to_unrelated_object() {
    let err = run_php_expect_error(
        r#"<?php
class A {
    private $x = 1;
    public function writeOther($o, $v) { $o->x = $v; }
}
class B {
    private $x = 2;
}
$a = new A();
$b = new B();
$a->writeOther($b, 99);
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("private"),
                "Expected private error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_private_property_separate_slots_regression() {
    assert_eq!(
        run_php(
            r#"<?php
class A {
    private $x = 1;
    public function a() { echo $this->x; }
}
class B extends A {
    private $x = 2;
    public function b() { echo $this->x; }
}
$o = new B();
$o->a();
$o->b();
"#
        ),
        "12"
    );
}

// ── P1 repro: Interface validation checks staticness ──

#[test]
fn test_interface_static_mismatch_rejected() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo();
}
class C implements I {
    public static function foo() {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from static mismatch in interface implementation"
    );
}

#[test]
fn test_interface_static_reverse_mismatch_rejected() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public static function foo();
}
class C implements I {
    public function foo() {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from non-static impl of static interface method"
    );
}

#[test]
fn test_interface_extra_required_param_rejected() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
interface I {
    public function foo($x);
}
class C implements I {
    public function foo($x, $y) {}
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from extra required param in interface implementation"
    );
}
