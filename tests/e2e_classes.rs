/// Tests for classes and objects (basic)
mod common;
use common::run_php;

#[test]
fn test_class_basic_property() {
    assert_eq!(run_php(r#"<?php
class Dog {
    public $name;
}
$d = new Dog();
$d->name = "Rex";
echo $d->name;
"#), "Rex");
}

#[test]
fn test_class_method() {
    assert_eq!(run_php(r#"<?php
class Dog {
    public $name;
    public function bark() {
        echo "Woof from " . $this->name;
    }
}
$d = new Dog();
$d->name = "Rex";
$d->bark();
"#), "Woof from Rex");
}

#[test]
fn test_class_method_with_params() {
    assert_eq!(run_php(r#"<?php
class Calculator {
    public function add($a, $b) {
        return $a + $b;
    }
}
$c = new Calculator();
echo $c->add(3, 4);
"#), "7");
}

#[test]
fn test_class_multiple_properties() {
    assert_eq!(run_php(r#"<?php
class Person {
    public $first;
    public $last;
}
$p = new Person();
$p->first = "John";
$p->last = "Doe";
echo $p->first . " " . $p->last;
"#), "John Doe");
}

#[test]
fn test_class_method_using_this() {
    assert_eq!(run_php(r#"<?php
class Counter {
    public $count;
    public function increment() {
        $this->count = $this->count + 1;
    }
    public function get() {
        return $this->count;
    }
}
$c = new Counter();
$c->count = 0;
$c->increment();
$c->increment();
$c->increment();
echo $c->get();
"#), "3");
}

#[test]
fn test_class_multiple_methods() {
    assert_eq!(run_php(r#"<?php
class Greeter {
    public $name;
    public function hello() {
        echo "Hello " . $this->name;
    }
    public function bye() {
        echo "Bye " . $this->name;
    }
}
$g = new Greeter();
$g->name = "World";
$g->hello();
echo " ";
$g->bye();
"#), "Hello World Bye World");
}

#[test]
fn test_class_multiple_instances() {
    assert_eq!(run_php(r#"<?php
class Box {
    public $value;
}
$a = new Box();
$a->value = 10;
$b = new Box();
$b->value = 20;
echo $a->value . " " . $b->value;
"#), "10 20");
}

#[test]
fn test_new_object_creates_instance() {
    assert_eq!(run_php(r#"<?php
class Foo {}
$f = new Foo();
echo "ok";
"#), "ok");
}

#[test]
fn test_class_method_return() {
    assert_eq!(run_php(r#"<?php
class Math {
    public function square($x) {
        return $x * $x;
    }
}
$m = new Math();
echo $m->square(7);
"#), "49");
}

#[test]
fn test_class_this_property_write_in_method() {
    assert_eq!(run_php(r#"<?php
class Setter {
    public $val;
    public function set($v) {
        $this->val = $v;
    }
}
$s = new Setter();
$s->set("hello");
echo $s->val;
"#), "hello");
}

#[test]
fn test_class_property_default_int() {
    assert_eq!(run_php(r#"<?php
class Config {
    public $timeout = 30;
}
$c = new Config();
echo $c->timeout;
"#), "30");
}

#[test]
fn test_class_property_default_string() {
    assert_eq!(run_php(r#"<?php
class Config {
    public $name = "default";
}
$c = new Config();
echo $c->name;
"#), "default");
}

#[test]
fn test_class_property_default_override() {
    assert_eq!(run_php(r#"<?php
class Config {
    public $x = 10;
}
$c = new Config();
$c->x = 42;
echo $c->x;
"#), "42");
}

#[test]
fn test_class_property_default_bool() {
    assert_eq!(run_php(r#"<?php
class Flags {
    public $active = true;
    public $deleted = false;
}
$f = new Flags();
echo $f->active;
"#), "1");
}

#[test]
fn test_class_property_no_default_is_null() {
    assert_eq!(run_php(r#"<?php
class Empty2 {
    public $x;
}
$e = new Empty2();
echo $e->x ?? "null";
"#), "null");
}
