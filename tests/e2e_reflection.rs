mod common;
use common::run_php;

#[test]
fn test_get_class_returns_class_name() {
    let out = run_php(r#"<?php
class Foo {}
$obj = new Foo();
echo get_class($obj);
"#);
    assert_eq!(out, "Foo");
}

#[test]
fn test_get_class_with_non_object_returns_false() {
    let out = run_php(r#"<?php
$result = get_class("hello");
var_dump($result);
"#);
    assert_eq!(out, "bool(false)\n");
}

#[test]
fn test_class_exists_true() {
    let out = run_php(r#"<?php
class MyClass {}
echo class_exists('MyClass') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}

#[test]
fn test_class_exists_false() {
    let out = run_php(r#"<?php
echo class_exists('NonExistent') ? 'yes' : 'no';
"#);
    assert_eq!(out, "no");
}

#[test]
fn test_method_exists_with_object_true() {
    let out = run_php(r#"<?php
class Bar {
    public function hello() {}
}
$obj = new Bar();
echo method_exists($obj, 'hello') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_with_object_false() {
    let out = run_php(r#"<?php
class Baz {
    public function hello() {}
}
$obj = new Baz();
echo method_exists($obj, 'nonexistent') ? 'yes' : 'no';
"#);
    assert_eq!(out, "no");
}

#[test]
fn test_method_exists_with_string_class_name() {
    let out = run_php(r#"<?php
class Qux {
    public function doStuff() {}
}
echo method_exists('Qux', 'doStuff') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_with_string_class_name_false() {
    let out = run_php(r#"<?php
class Corge {
    public function doStuff() {}
}
echo method_exists('Corge', 'missing') ? 'yes' : 'no';
"#);
    assert_eq!(out, "no");
}

// -- method_exists with inheritance --

#[test]
fn test_method_exists_inherited_method() {
    let out = run_php(r#"<?php
class A {
    public function foo() {}
}
class B extends A {}
echo method_exists('B', 'foo') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_inherited_on_object() {
    let out = run_php(r#"<?php
class Parent1 {
    public function parentMethod() {}
}
class Child1 extends Parent1 {}
$c = new Child1();
echo method_exists($c, 'parentMethod') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_deep_inheritance() {
    let out = run_php(r#"<?php
class GrandParent1 {
    public function deep() {}
}
class Parent2 extends GrandParent1 {}
class Child2 extends Parent2 {}
echo method_exists('Child2', 'deep') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}

// -- class_exists excludes interfaces and traits --

#[test]
fn test_class_exists_interface_false() {
    let out = run_php(r#"<?php
interface MyInterface {}
echo class_exists('MyInterface') ? 'yes' : 'no';
"#);
    assert_eq!(out, "no");
}

#[test]
fn test_class_exists_trait_false() {
    let out = run_php(r#"<?php
trait MyTrait {}
echo class_exists('MyTrait') ? 'yes' : 'no';
"#);
    assert_eq!(out, "no");
}

#[test]
fn test_class_exists_real_class_still_true() {
    let out = run_php(r#"<?php
interface I {}
trait T {}
class C implements I { use T; }
echo class_exists('C') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}

// -- method_exists with traits --

#[test]
fn test_method_exists_trait_method() {
    let out = run_php(r#"<?php
trait Greetable {
    public function greet() { return "hi"; }
}
class Hello {
    use Greetable;
}
echo method_exists('Hello', 'greet') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}

#[test]
fn test_method_exists_trait_method_on_object() {
    let out = run_php(r#"<?php
trait Greetable {
    public function greet() { return "hi"; }
}
class Hello {
    use Greetable;
}
$h = new Hello();
echo method_exists($h, 'greet') ? 'yes' : 'no';
"#);
    assert_eq!(out, "yes");
}
