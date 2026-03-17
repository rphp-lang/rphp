mod common;
use common::run_php;

#[test]
fn test_clone_basic_properties_independent() {
    let output = run_php(r#"<?php
class Foo {
    public $x = 1;
    public $y = 2;
}
$a = new Foo();
$b = clone $a;
$b->x = 10;
echo $a->x . "\n";
echo $b->x . "\n";
echo $a->y . "\n";
echo $b->y . "\n";
"#);
    assert_eq!(output, "1\n10\n2\n2\n");
}

#[test]
fn test_clone_does_not_affect_original() {
    let output = run_php(r#"<?php
class Point {
    public $x;
    public $y;
    public function __construct($x, $y) {
        $this->x = $x;
        $this->y = $y;
    }
}
$p1 = new Point(3, 4);
$p2 = clone $p1;
$p2->x = 99;
$p2->y = 100;
echo $p1->x . "," . $p1->y . "\n";
echo $p2->x . "," . $p2->y . "\n";
"#);
    assert_eq!(output, "3,4\n99,100\n");
}

#[test]
fn test_clone_magic_method_called() {
    let output = run_php(r#"<?php
class WithClone {
    public $name;
    public $cloned = false;
    public function __construct($name) {
        $this->name = $name;
    }
    public function __clone() {
        $this->cloned = true;
        $this->name = $this->name . "_copy";
    }
}
$a = new WithClone("original");
$b = clone $a;
echo $a->name . "\n";
echo $a->cloned . "\n";
echo $b->name . "\n";
echo $b->cloned . "\n";
"#);
    // $a should be unaffected; $b should have __clone modifications
    assert_eq!(output, "original\n\noriginal_copy\n1\n");
}

#[test]
fn test_clone_shallow_nested_objects_shared() {
    let output = run_php(r#"<?php
class Inner {
    public $val = 10;
}
class Outer {
    public $inner;
    public function __construct() {
        $this->inner = new Inner();
    }
}
$a = new Outer();
$b = clone $a;
// Shallow clone: $b->inner is the same object as $a->inner
$b->inner->val = 42;
echo $a->inner->val . "\n";
echo $b->inner->val . "\n";
"#);
    // Both should be 42 because the inner object is shared (shallow clone)
    assert_eq!(output, "42\n42\n");
}

#[test]
fn test_clone_with_deep_copy_via_clone_method() {
    let output = run_php(r#"<?php
class Inner {
    public $val;
    public function __construct($v) {
        $this->val = $v;
    }
}
class Outer {
    public $inner;
    public function __construct($v) {
        $this->inner = new Inner($v);
    }
    public function __clone() {
        $this->inner = clone $this->inner;
    }
}
$a = new Outer(5);
$b = clone $a;
$b->inner->val = 99;
echo $a->inner->val . "\n";
echo $b->inner->val . "\n";
"#);
    // Deep clone via __clone — inner objects should be independent
    assert_eq!(output, "5\n99\n");
}

#[test]
fn test_clone_exception_in_clone_method() {
    let output = run_php(r#"<?php
class NoClone {
    public function __clone() {
        throw new Exception("Clone not allowed");
    }
}
$a = new NoClone();
try {
    $b = clone $a;
    echo "no_exception";
} catch (Exception $e) {
    echo "caught:" . $e->getMessage();
}
"#);
    assert_eq!(output, "caught:Clone not allowed");
}
