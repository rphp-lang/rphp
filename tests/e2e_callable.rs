mod common;
use common::run_php;

// -- call_user_func with string callback --

#[test]
fn test_call_user_func_strtoupper() {
    let out = run_php(r#"<?php
echo call_user_func('strtoupper', 'hello');
"#);
    assert_eq!(out, "HELLO");
}

#[test]
fn test_call_user_func_strlen() {
    let out = run_php(r#"<?php
echo call_user_func('strlen', 'abc');
"#);
    assert_eq!(out, "3");
}

#[test]
fn test_call_user_func_user_function() {
    let out = run_php(r#"<?php
function double($x) { return $x * 2; }
echo call_user_func('double', 5);
"#);
    assert_eq!(out, "10");
}

#[test]
fn test_call_user_func_multiple_args() {
    let out = run_php(r#"<?php
function add($a, $b) { return $a + $b; }
echo call_user_func('add', 3, 7);
"#);
    assert_eq!(out, "10");
}

// -- is_callable with string --

#[test]
fn test_is_callable_existing_function() {
    let out = run_php(r#"<?php
echo is_callable('strtoupper') ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_is_callable_nonexistent_function() {
    let out = run_php(r#"<?php
echo is_callable('nonexistent_func') ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_user_function() {
    let out = run_php(r#"<?php
function myFunc() {}
echo is_callable('myFunc') ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

// -- is_callable with non-callable values --

#[test]
fn test_is_callable_integer() {
    let out = run_php(r#"<?php
echo is_callable(42) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_null() {
    let out = run_php(r#"<?php
echo is_callable(null) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

// -- call_user_func with closure --

#[test]
fn test_call_user_func_closure() {
    let out = run_php(r#"<?php
$fn = function($x) { return $x * 3; };
echo call_user_func($fn, 4);
"#);
    assert_eq!(out, "12");
}

#[test]
fn test_is_callable_closure() {
    let out = run_php(r#"<?php
$fn = function() { return 1; };
echo is_callable($fn) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

// -- call_user_func / is_callable with array callback --

#[test]
fn test_call_user_func_array_method() {
    let out = run_php(r#"<?php
class Greeter {
    public function greet($name) {
        return "Hello, " . $name;
    }
}
$g = new Greeter();
echo call_user_func([$g, 'greet'], 'World');
"#);
    assert_eq!(out, "Hello, World");
}

#[test]
fn test_is_callable_array_method() {
    let out = run_php(r#"<?php
class Foo {
    public function bar() {}
}
$f = new Foo();
echo is_callable([$f, 'bar']) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_is_callable_array_nonexistent_method() {
    let out = run_php(r#"<?php
class Foo {
    public function bar() {}
}
$f = new Foo();
echo is_callable([$f, 'nonexistent']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

// -- call_user_func / is_callable with static method ["ClassName", "method"] --

#[test]
fn test_call_user_func_static_method() {
    let out = run_php(r#"<?php
class Math {
    public static function double($x) {
        return $x * 2;
    }
}
echo call_user_func(['Math', 'double'], 5);
"#);
    assert_eq!(out, "10");
}

#[test]
fn test_is_callable_static_method() {
    let out = run_php(r#"<?php
class Util {
    public static function helper() { return 1; }
}
echo is_callable(['Util', 'helper']) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_is_callable_static_nonexistent_method() {
    let out = run_php(r#"<?php
class Util {
    public static function helper() { return 1; }
}
echo is_callable(['Util', 'nope']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

// -- visibility enforcement --

#[test]
fn test_is_callable_private_method_from_outside() {
    let out = run_php(r#"<?php
class Secret {
    private function hidden() { return 42; }
}
$s = new Secret();
echo is_callable([$s, 'hidden']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_protected_method_from_outside() {
    let out = run_php(r#"<?php
class Base {
    protected function internal() { return 1; }
}
$b = new Base();
echo is_callable([$b, 'internal']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_is_callable_non_static_via_string_class() {
    // ["ClassName", "instanceMethod"] should be false — method is not static
    let out = run_php(r#"<?php
class Foo {
    public function bar() { return 1; }
}
echo is_callable(['Foo', 'bar']) ? 'true' : 'false';
"#);
    assert_eq!(out, "false");
}

#[test]
fn test_call_user_func_private_method_throws() {
    let out = run_php(r#"<?php
class Secret {
    private function hidden() { return 42; }
}
$s = new Secret();
try {
    call_user_func([$s, 'hidden']);
    echo "no_error";
} catch (\Throwable $e) {
    echo "caught";
}
"#);
    assert_eq!(out, "caught");
}

// -- inherited method callbacks --

#[test]
fn test_is_callable_inherited_instance_method() {
    let out = run_php(r#"<?php
class A {
    public function foo() { return 42; }
}
class B extends A {}
$b = new B();
echo is_callable([$b, 'foo']) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_call_user_func_inherited_instance_method() {
    let out = run_php(r#"<?php
class A {
    public function foo() { return 42; }
}
class B extends A {}
$b = new B();
echo call_user_func([$b, 'foo']);
"#);
    assert_eq!(out, "42");
}

#[test]
fn test_is_callable_inherited_static_method() {
    let out = run_php(r#"<?php
class A {
    public static function bar() { return 99; }
}
class B extends A {}
echo is_callable(['B', 'bar']) ? 'true' : 'false';
"#);
    assert_eq!(out, "true");
}

#[test]
fn test_call_user_func_inherited_static_method() {
    let out = run_php(r#"<?php
class A {
    public static function bar() { return 99; }
}
class B extends A {}
echo call_user_func(['B', 'bar']);
"#);
    assert_eq!(out, "99");
}
