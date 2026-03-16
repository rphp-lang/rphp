/// Tests for class inheritance (extends)
mod common;
use common::run_php;

#[test]
fn test_extends_basic() {
    assert_eq!(run_php(r#"<?php
class Animal {
    public $name;
    public function speak() {
        return "...";
    }
}
class Dog extends Animal {
    public function speak() {
        return "Woof";
    }
}
$d = new Dog();
$d->name = "Rex";
echo $d->name . " says " . $d->speak();
"#), "Rex says Woof");
}

#[test]
fn test_extends_inherits_method() {
    assert_eq!(run_php(r#"<?php
class Base {
    public function hello() {
        return "Hello";
    }
}
class Child extends Base {}
$c = new Child();
echo $c->hello();
"#), "Hello");
}

#[test]
fn test_extends_inherits_property_default() {
    assert_eq!(run_php(r#"<?php
class Base {
    public $x = 42;
}
class Child extends Base {}
$c = new Child();
echo $c->x;
"#), "42");
}

#[test]
fn test_extends_child_overrides_method() {
    assert_eq!(run_php(r#"<?php
class Base {
    public function value() {
        return "base";
    }
}
class Child extends Base {
    public function value() {
        return "child";
    }
}
$b = new Base();
$c = new Child();
echo $b->value() . " " . $c->value();
"#), "base child");
}

#[test]
fn test_extends_child_adds_property() {
    assert_eq!(run_php(r#"<?php
class Base {
    public $x = 1;
}
class Child extends Base {
    public $y = 2;
}
$c = new Child();
echo $c->x . " " . $c->y;
"#), "1 2");
}

#[test]
fn test_extends_constructor_inherited() {
    assert_eq!(run_php(r#"<?php
class Base {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}
class Child extends Base {}
$c = new Child("test");
echo $c->name;
"#), "test");
}

#[test]
fn test_extends_constructor_overridden() {
    assert_eq!(run_php(r#"<?php
class Base {
    public $x;
    public function __construct($x) {
        $this->x = $x;
    }
}
class Child extends Base {
    public $y;
    public function __construct($x, $y) {
        $this->x = $x;
        $this->y = $y;
    }
}
$c = new Child(1, 2);
echo $c->x . " " . $c->y;
"#), "1 2");
}

#[test]
fn test_extends_three_levels() {
    assert_eq!(run_php(r#"<?php
class A {
    public function who() { return "A"; }
}
class B extends A {}
class C extends B {
    public function who() { return "C"; }
}
$a = new A();
$b = new B();
$c = new C();
echo $a->who() . $b->who() . $c->who();
"#), "AAC");
}

#[test]
fn test_extends_method_uses_this() {
    assert_eq!(run_php(r#"<?php
class Base {
    public $name;
    public function greet() {
        return "Hi " . $this->name;
    }
}
class Child extends Base {
    public function __construct($name) {
        $this->name = $name;
    }
}
$c = new Child("PHP");
echo $c->greet();
"#), "Hi PHP");
}

#[test]
fn test_extends_grandchild_inherits_grandparent_method() {
    // Regression: transitive inheritance must work across 3+ levels
    assert_eq!(run_php(r#"<?php
class A {
    public function foo() { return "A"; }
}
class B extends A {}
class C extends B {}
$c = new C();
echo $c->foo();
"#), "A");
}

#[test]
fn test_extends_grandchild_inherits_constructor() {
    // Regression: constructor must be inherited transitively
    assert_eq!(run_php(r#"<?php
class A {
    public $x;
    public function __construct($x) {
        $this->x = $x;
    }
}
class B extends A {}
class C extends B {}
$c = new C(42);
echo $c->x;
"#), "42");
}
