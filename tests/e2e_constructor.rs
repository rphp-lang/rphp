/// Tests for __construct() constructor
mod common;
use common::run_php;

#[test]
fn test_constructor_basic() {
    assert_eq!(run_php(r#"<?php
class Dog {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}
$d = new Dog("Rex");
echo $d->name;
"#), "Rex");
}

#[test]
fn test_constructor_multiple_args() {
    assert_eq!(run_php(r#"<?php
class Point {
    public $x;
    public $y;
    public function __construct($x, $y) {
        $this->x = $x;
        $this->y = $y;
    }
}
$p = new Point(3, 4);
echo $p->x . "," . $p->y;
"#), "3,4");
}

#[test]
fn test_constructor_with_method() {
    assert_eq!(run_php(r#"<?php
class Greeter {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
    public function greet() {
        return "Hello " . $this->name;
    }
}
$g = new Greeter("World");
echo $g->greet();
"#), "Hello World");
}

#[test]
fn test_constructor_no_args() {
    assert_eq!(run_php(r#"<?php
class Counter {
    public $count;
    public function __construct() {
        $this->count = 0;
    }
    public function increment() {
        $this->count = $this->count + 1;
    }
}
$c = new Counter();
$c->increment();
$c->increment();
echo $c->count;
"#), "2");
}

#[test]
fn test_constructor_default_overridden() {
    assert_eq!(run_php(r#"<?php
class Config {
    public $timeout = 30;
    public function __construct($t) {
        $this->timeout = $t;
    }
}
$c = new Config(60);
echo $c->timeout;
"#), "60");
}

#[test]
fn test_no_constructor_no_args() {
    // Class without constructor — new still works
    assert_eq!(run_php(r#"<?php
class Empty2 {}
$e = new Empty2();
echo "ok";
"#), "ok");
}

#[test]
fn test_multiple_objects_different_constructor_args() {
    assert_eq!(run_php(r#"<?php
class Box {
    public $value;
    public function __construct($v) {
        $this->value = $v;
    }
}
$a = new Box(10);
$b = new Box(20);
echo $a->value . " " . $b->value;
"#), "10 20");
}

#[test]
fn test_no_constructor_with_args_silently_ignored() {
    // PHP evaluates arg expressions (side effects run) but ignores values
    // when class has no __construct
    assert_eq!(run_php(r#"<?php
class Foo {}
function side() { echo "S"; return 1; }
$f = new Foo(side());
echo "X";
"#), "SX");
}

#[test]
fn test_no_constructor_negative_cache_keeps_argument_side_effects() {
    assert_eq!(run_php(r#"<?php
class PlainBox { public $value = 7; }
$sum = 0;
for ($i = 0; $i < 5; $i++) {
    $box = new PlainBox($sum = $sum + 1);
}
echo $sum . ':' . $box->value;
"#), "5:7");
}
