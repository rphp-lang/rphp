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
