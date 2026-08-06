/// Tests for interface support and visibility enforcement
mod common;
use common::{run_php, run_php_expect_error};

// ── Interface basics ──

#[test]
fn test_interface_declaration() {
    assert_eq!(
        run_php(
            r#"<?php
interface Greetable {
    public function greet();
}
class Person implements Greetable {
    public function greet() {
        echo "Hello!";
    }
}
$p = new Person();
$p->greet();
"#
        ),
        "Hello!"
    );
}

#[test]
fn test_interface_instanceof() {
    assert_eq!(
        run_php(
            r#"<?php
interface Printable {
    public function display();
}
class Document implements Printable {
    public function display() {
        echo "doc";
    }
}
$d = new Document();
echo $d instanceof Printable ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_interface_instanceof_negative() {
    assert_eq!(
        run_php(
            r#"<?php
interface Printable {
    public function display();
}
interface Editable {
    public function edit();
}
class Document implements Printable {
    public function display() {
        echo "doc";
    }
}
$d = new Document();
echo $d instanceof Editable ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_multiple_implements() {
    assert_eq!(
        run_php(
            r#"<?php
interface Readable {
    public function read();
}
interface Writable {
    public function write();
}
class File implements Readable, Writable {
    public function read() { echo "R"; }
    public function write() { echo "W"; }
}
$f = new File();
$f->read();
$f->write();
echo " ";
echo ($f instanceof Readable ? "r" : "n") . ($f instanceof Writable ? "w" : "n");
"#
        ),
        "RW rw"
    );
}

#[test]
fn test_class_extends_and_implements() {
    assert_eq!(
        run_php(
            r#"<?php
interface Describable {
    public function describe();
}
class Animal {
    public function type() { echo "animal"; }
}
class Dog extends Animal implements Describable {
    public function describe() { echo "woof"; }
}
$d = new Dog();
$d->describe();
echo " ";
echo ($d instanceof Animal ? "a" : "n") . ($d instanceof Describable ? "d" : "n");
"#
        ),
        "woof ad"
    );
}

// ── Throwable as interface (PHP 8 compatible) ──

#[test]
fn test_throwable_is_interface() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("test");
} catch (Throwable $e) {
    echo $e->getMessage();
}
"#
        ),
        "test"
    );
}

#[test]
fn test_error_instanceof_throwable() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Error("oops");
} catch (Throwable $e) {
    echo $e->getMessage();
}
"#
        ),
        "oops"
    );
}

#[test]
fn test_typeerror_instanceof_throwable() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new TypeError("type fail");
} catch (Throwable $e) {
    echo $e->getMessage();
}
"#
        ),
        "type fail"
    );
}

#[test]
fn test_exception_not_instanceof_error() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Exception("x");
echo $e instanceof Error ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_error_not_instanceof_exception() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Error("x");
echo $e instanceof Exception ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_exception_instanceof_throwable() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Exception("x");
echo $e instanceof Throwable ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_error_instanceof_throwable_via_instanceof() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Error("x");
echo $e instanceof Throwable ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_typeerror_instanceof_throwable_via_instanceof() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new TypeError("x");
echo ($e instanceof Throwable ? "T" : "n") . ($e instanceof Error ? "E" : "n");
"#
        ),
        "TE"
    );
}

// ── Visibility enforcement ──

#[test]
fn test_public_method_accessible() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {
    public function bar() { echo "ok"; }
}
$f = new Foo();
$f->bar();
"#
        ),
        "ok"
    );
}

#[test]
fn test_private_method_from_outside() {
    let err = run_php_expect_error(
        r#"<?php
class Foo {
    private function secret() { echo "nope"; }
}
$f = new Foo();
$f->secret();
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
fn test_protected_method_from_outside() {
    let err = run_php_expect_error(
        r#"<?php
class Foo {
    protected function hidden() { echo "nope"; }
}
$f = new Foo();
$f->hidden();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("protected"),
                "Expected protected error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_private_method_from_same_class() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {
    private function secret() { return "ok"; }
    public function reveal() { echo $this->secret(); }
}
$f = new Foo();
$f->reveal();
"#
        ),
        "ok"
    );
}

#[test]
fn test_protected_method_from_child() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    protected function hidden() { return "found"; }
}
class Child extends Base {
    public function show() { echo $this->hidden(); }
}
$c = new Child();
$c->show();
"#
        ),
        "found"
    );
}

#[test]
fn test_private_property_from_outside() {
    let err = run_php_expect_error(
        r#"<?php
class Foo {
    private $secret = "hidden";
}
$f = new Foo();
echo $f->secret;
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
fn test_protected_property_from_outside() {
    let err = run_php_expect_error(
        r#"<?php
class Foo {
    protected $hidden = "nope";
}
$f = new Foo();
echo $f->hidden;
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("protected"),
                "Expected protected error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_public_property_accessible() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {
    public $name = "world";
}
$f = new Foo();
echo $f->name;
"#
        ),
        "world"
    );
}

#[test]
fn test_protected_property_from_child() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    protected $val = 42;
}
class Child extends Base {
    public function show() { echo $this->val; }
}
$c = new Child();
$c->show();
"#
        ),
        "42"
    );
}

#[test]
fn test_private_property_write_from_outside() {
    let err = run_php_expect_error(
        r#"<?php
class Foo {
    private $x = 0;
}
$f = new Foo();
$f->x = 99;
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

// ── Interface extends interface ──

#[test]
fn test_interface_extends_interface() {
    assert_eq!(
        run_php(
            r#"<?php
interface A {
    public function foo();
}
interface B extends A {
    public function bar();
}
class C implements B {
    public function foo() { echo "F"; }
    public function bar() { echo "B"; }
}
$c = new C();
$c->foo();
$c->bar();
echo " ";
echo ($c instanceof A ? "a" : "n") . ($c instanceof B ? "b" : "n");
"#
        ),
        "FB ab"
    );
}

// ── Catch with Throwable interface ──

#[test]
fn test_catch_throwable_catches_both() {
    assert_eq!(
        run_php(
            r#"<?php
function test1() {
    try { throw new Exception("ex"); } catch (Throwable $e) { echo "E"; }
}
function test2() {
    try { throw new Error("err"); } catch (Throwable $e) { echo "R"; }
}
test1();
test2();
"#
        ),
        "ER"
    );
}

#[test]
fn test_catch_error_directly() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Error("err");
} catch (Error $e) {
    echo "correct";
}
"#
        ),
        "correct"
    );
}

// ── P1: Interface not instantiable ──

#[test]
fn test_interface_not_instantiable() {
    let err = run_php_expect_error(
        r#"<?php
interface I {
    public function foo();
}
$x = new I();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Cannot instantiate interface"), "got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_throwable_not_instantiable() {
    let err = run_php_expect_error(
        r#"<?php
$x = new Throwable();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Cannot instantiate interface"), "got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

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
