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
fn test_class_scalar_long_method_nested_calls() {
    assert_eq!(run_php(r#"<?php
class Calculator {
    public function add($a, $b) { return $a + $b; }
    public function mul($a, $b) { return $a * $b; }
}
$c = new Calculator();
echo $c->add(2, $c->mul(3, 4));
"#), "14");
}

#[test]
fn test_class_composed_scalar_call_guards_polymorphic_dispatch() {
    assert_eq!(run_php(r#"<?php
class AddMath {
    public function combine($a, $b) { return $a + $b; }
    public function inner($a, $b) { return $a * $b; }
}
class SubMath {
    public function combine($a, $b) { return $a - $b; }
    public function inner($a, $b) { return $a + $b; }
}
function calculate($math, $value) {
    return $math->combine($value, $math->inner($value, 2));
}
$add = new AddMath();
$sub = new SubMath();
echo calculate($add, 3) . ':' . calculate($add, 4) . '|';
echo calculate($sub, 3) . ':' . calculate($sub, 4) . '|';
echo calculate($add, 5);
"#), "9:12|-2:-2|15");
}

#[test]
fn test_object_long_method_side_exits_across_property_layouts_and_types() {
    assert_eq!(run_php(r#"<?php
class RequestA {
    public $level;
    public $subtotal;
}
class RequestB {
    public $subtotal;
    public $level;
}
class RequestC {
    public $level;
    public $subtotal;
}
class Policy {
    public function rate($request) {
        $rate = 150;
        if ($request->level >= 3) $rate = $rate + 250;
        if ($request->subtotal >= 20000) $rate = $rate + 175;
        return $rate;
    }
}
function invoke($policy, $request) {
    return $policy->rate($request);
}
$policy = new Policy();
$a = new RequestA(); $a->level = 4; $a->subtotal = 30000;
$b = new RequestB(); $b->level = 1; $b->subtotal = 30000;
$c = new RequestC(); $c->level = 4.0; $c->subtotal = 100.0;
echo invoke($policy, $a) . ':' . invoke($policy, $a) . '|';
echo invoke($policy, $b) . ':' . invoke($policy, $b) . '|';
echo invoke($policy, $c) . ':' . invoke($policy, $c) . '|';
echo invoke($policy, $a);
"#), "575:575|325:325|400:400|575");
}

#[test]
fn test_object_long_method_handles_string_branches_and_intdiv() {
    assert_eq!(run_php(r#"<?php
class TaxPolicy {
    public function amount($net, $region) {
        if ($region == 'EU') return intdiv($net * 2100, 10000);
        if ($region == 'US') return intdiv($net * 725, 10000);
        return intdiv($net * 1200, 10000);
    }
}
function tax($policy, $net, $region) {
    return $policy->amount($net, $region);
}
function taxByReference($policy, $net, &$region) {
    return $policy->amount($net, $region);
}
$policy = new TaxPolicy();
$eu = 'EU';
echo tax($policy, 10000, 'EU') . ':';
echo tax($policy, 10000, 'US') . ':';
echo tax($policy, 10000, 'ROW') . ':';
echo taxByReference($policy, 10000, $eu);
"#), "2100:725:1200:2100");
}

#[test]
fn test_object_long_property_argument_rechecks_layout_and_dynamic_fallback() {
    assert_eq!(run_php(r#"<?php
class TaxPolicy {
    public function amount($net, $region) {
        if ($region == 'EU') return intdiv($net * 2100, 10000);
        if ($region == 'US') return intdiv($net * 725, 10000);
        return intdiv($net * 1200, 10000);
    }
}
class RequestA { public $region; }
class RequestB { public $padding; public $region; }
class DynamicRequest {}
function quoteTax($policy, $request) {
    return $policy->amount(10000, $request->region);
}
$policy = new TaxPolicy();
$a = new RequestA(); $a->region = 'EU';
$b = new RequestB(); $b->region = 'US';
$dynamic = new DynamicRequest(); $dynamic->region = 'ROW';
echo quoteTax($policy, $a) . ':' . quoteTax($policy, $a) . '|';
echo quoteTax($policy, $b) . ':' . quoteTax($policy, $b) . '|';
echo quoteTax($policy, $dynamic) . ':' . quoteTax($policy, $dynamic) . '|';
echo quoteTax($policy, $a);
"#), "2100:2100|725:725|1200:1200|2100");
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

#[test]
fn test_borrowed_object_parameter_materializes_before_nested_by_ref_rebind() {
    assert_eq!(run_php(r#"<?php
class BorrowBox {
    public $value;
    public function __construct($value) { $this->value = $value; }
}
function replaceBorrowBox(&$box) {
    $box = new BorrowBox(9);
}
function observeAndReplaceBorrowBox($box) {
    $before = $box->value;
    replaceBorrowBox($box);
    return $before . ':' . $box->value;
}
$original = new BorrowBox(3);
for ($i = 0; $i < 20; $i++) {
    $last = observeAndReplaceBorrowBox($original);
}
echo $last . '|' . $original->value;
"#), "3:9|3");
}
