/// Tests for instanceof operator and catch type matching
mod common;
use common::{run_php, run_php_expect_error};

// ── instanceof operator ──

#[test]
fn test_instanceof_basic() {
    assert_eq!(run_php(r#"<?php
class Foo {}
$f = new Foo();
echo $f instanceof Foo ? "yes" : "no";
"#), "yes");
}

#[test]
fn test_instanceof_wrong_class() {
    assert_eq!(run_php(r#"<?php
class Foo {}
class Bar {}
$f = new Foo();
echo $f instanceof Bar ? "yes" : "no";
"#), "no");
}

#[test]
fn test_instanceof_parent_class() {
    assert_eq!(run_php(r#"<?php
class Animal {}
class Dog extends Animal {}
$d = new Dog();
echo ($d instanceof Animal ? "yes" : "no") . " " . ($d instanceof Dog ? "yes" : "no");
"#), "yes yes");
}

#[test]
fn test_instanceof_grandparent() {
    assert_eq!(run_php(r#"<?php
class A {}
class B extends A {}
class C extends B {}
$c = new C();
echo $c instanceof A ? "yes" : "no";
"#), "yes");
}

#[test]
fn test_instanceof_non_object() {
    assert_eq!(run_php(r#"<?php
class Foo {}
$x = 42;
echo $x instanceof Foo ? "yes" : "no";
"#), "no");
}

#[test]
fn test_instanceof_parent_not_child() {
    assert_eq!(run_php(r#"<?php
class Animal {}
class Dog extends Animal {}
$a = new Animal();
echo $a instanceof Dog ? "yes" : "no";
"#), "no");
}

// ── catch type matching ──

#[test]
fn test_catch_specific_exception_class() {
    assert_eq!(run_php(r#"<?php
class MyError {}
try {
    throw new MyError();
} catch (MyError $e) {
    echo "caught MyError";
}
"#), "caught MyError");
}

#[test]
fn test_catch_parent_exception_class() {
    assert_eq!(run_php(r#"<?php
class BaseError {}
class ChildError extends BaseError {}
try {
    throw new ChildError();
} catch (BaseError $e) {
    echo "caught BaseError";
}
"#), "caught BaseError");
}

#[test]
fn test_catch_wrong_type_falls_through() {
    let err = run_php_expect_error(r#"<?php
class FooError {}
class BarError {}
try {
    throw new FooError();
} catch (BarError $e) {
    echo "caught BarError";
}
"#);
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Uncaught"), "Expected uncaught exception, got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_catch_multiple_clauses_second_matches() {
    assert_eq!(run_php(r#"<?php
class FooError {}
class BarError {}
try {
    throw new BarError();
} catch (FooError $e) {
    echo "caught Foo";
} catch (BarError $e) {
    echo "caught Bar";
}
"#), "caught Bar");
}

#[test]
fn test_catch_multiple_clauses_first_matches() {
    assert_eq!(run_php(r#"<?php
class FooError {}
class BarError {}
try {
    throw new FooError();
} catch (FooError $e) {
    echo "caught Foo";
} catch (BarError $e) {
    echo "caught Bar";
}
"#), "caught Foo");
}

#[test]
fn test_catch_exception_catches_string_throw() {
    // "Exception" type catches non-object throws (backward compat)
    assert_eq!(run_php(r#"<?php
try {
    throw "string error";
} catch (Exception $e) {
    echo "caught: " . $e;
}
"#), "caught: string error");
}

#[test]
fn test_catch_specific_type_does_not_catch_string() {
    // Specific class type should NOT catch a string throw
    let err = run_php_expect_error(r#"<?php
class MyError {}
try {
    throw "string error";
} catch (MyError $e) {
    echo "caught";
}
"#);
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
    let err = run_php_expect_error(r#"<?php
class FooError {}
class BarError {}
try {
    throw new FooError();
} catch (BarError $e) {
    echo "caught";
} finally {
    echo "F";
}
"#);
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Uncaught"), "Expected uncaught, got: {}", msg);
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_instanceof_in_conditional() {
    assert_eq!(run_php(r#"<?php
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
"#), "cat dog");
}
