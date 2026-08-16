/// Tests for instanceof operator and catch type matching
mod common;
use common::{run_php, run_php_expect_error};

// ── instanceof operator ──

#[test]
fn test_instanceof_basic() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
$f = new Foo();
echo $f instanceof Foo ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_instanceof_wrong_class() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
class Bar {}
$f = new Foo();
echo $f instanceof Bar ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_instanceof_parent_class() {
    assert_eq!(
        run_php(
            r#"<?php
class Animal {}
class Dog extends Animal {}
$d = new Dog();
echo ($d instanceof Animal ? "yes" : "no") . " " . ($d instanceof Dog ? "yes" : "no");
"#
        ),
        "yes yes"
    );
}

#[test]
fn test_instanceof_grandparent() {
    assert_eq!(
        run_php(
            r#"<?php
class A {}
class B extends A {}
class C extends B {}
$c = new C();
echo $c instanceof A ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_instanceof_non_object() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
$x = 42;
echo $x instanceof Foo ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_instanceof_parent_not_child() {
    assert_eq!(
        run_php(
            r#"<?php
class Animal {}
class Dog extends Animal {}
$a = new Animal();
echo $a instanceof Dog ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_instanceof_resolves_self_static_and_trait_scopes() {
    assert_eq!(
        run_php(
            r#"<?php
class InstanceofSelfBase {
    public static function inspect($value) {
        echo ($value instanceof self ? 's' : '-') . ($value instanceof static ? 'l' : '-');
    }
}
class InstanceofSelfChild extends InstanceofSelfBase {}
trait InstanceofSelfTrait {
    public function inspectTrait($value) { echo $value instanceof self ? 't' : '-'; }
}
class InstanceofTraitConsumer { use InstanceofSelfTrait; }
InstanceofSelfBase::inspect(new InstanceofSelfBase());
echo '|';
InstanceofSelfChild::inspect(new InstanceofSelfBase());
echo '|';
(new InstanceofTraitConsumer())->inspectTrait(new InstanceofTraitConsumer());
"#
        ),
        "sl|s-|t"
    );
}

// ── catch type matching ──

#[test]
fn test_catch_specific_exception_class() {
    assert_eq!(
        run_php(
            r#"<?php
class MyError extends Exception {}
try {
    throw new MyError("test");
} catch (MyError $e) {
    echo "caught MyError";
}
"#
        ),
        "caught MyError"
    );
}

#[test]
fn test_catch_parent_exception_class() {
    assert_eq!(
        run_php(
            r#"<?php
class BaseError extends Exception {}
class ChildError extends BaseError {}
try {
    throw new ChildError("test");
} catch (BaseError $e) {
    echo "caught BaseError";
}
"#
        ),
        "caught BaseError"
    );
}

#[test]
fn test_catch_wrong_type_falls_through() {
    let err = run_php_expect_error(
        r#"<?php
class FooError extends Exception {}
class BarError extends Exception {}
try {
    throw new FooError("test");
} catch (BarError $e) {
    echo "caught BarError";
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught"),
                "Expected uncaught exception, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_catch_multiple_clauses_second_matches() {
    assert_eq!(
        run_php(
            r#"<?php
class FooError extends Exception {}
class BarError extends Exception {}
try {
    throw new BarError("test");
} catch (FooError $e) {
    echo "caught Foo";
} catch (BarError $e) {
    echo "caught Bar";
}
"#
        ),
        "caught Bar"
    );
}

#[test]
fn test_catch_multiple_clauses_first_matches() {
    assert_eq!(
        run_php(
            r#"<?php
class FooError extends Exception {}
class BarError extends Exception {}
try {
    throw new FooError("test");
} catch (FooError $e) {
    echo "caught Foo";
} catch (BarError $e) {
    echo "caught Bar";
}
"#
        ),
        "caught Foo"
    );
}

#[test]
fn test_catch_exception_catches_exception_throw() {
    // Exception type catches Exception objects
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("error msg");
} catch (Exception $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: error msg"
    );
}

#[test]
fn test_catch_specific_type_does_not_catch_exception() {
    // Specific class type should NOT catch an Exception throw (unless it's that type)
    let err = run_php_expect_error(
        r#"<?php
class MyError extends Error {}
try {
    throw new Exception("error");
} catch (MyError $e) {
    echo "caught";
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Uncaught"), "Expected uncaught, got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_catch_with_finally_type_mismatch() {
    // Exception doesn't match catch type, finally still runs, then uncaught
    let err = run_php_expect_error(
        r#"<?php
class FooError extends Exception {}
class BarError extends Exception {}
try {
    throw new FooError("test");
} catch (BarError $e) {
    echo "caught";
} finally {
    echo "F";
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Uncaught"), "Expected uncaught, got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_instanceof_in_conditional() {
    assert_eq!(
        run_php(
            r#"<?php
class Cat {}
class Dog {}
function describe($animal) {
    if ($animal instanceof Cat) {
        return "cat";
    }
    if ($animal instanceof Dog) {
        return "dog";
    }
    return "unknown";
}
echo describe(new Cat()) . " " . describe(new Dog());
"#
        ),
        "cat dog"
    );
}

// ── throw validation ──

#[test]
fn test_throw_non_throwable_class_is_fatal() {
    let err = run_php_expect_error(
        r#"<?php
class NotThrowable {}
throw new NotThrowable();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert_eq!(
                msg,
                "Uncaught Error: Cannot throw objects that do not implement Throwable"
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}
