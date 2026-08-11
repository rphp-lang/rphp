/// Tests for class inheritance (extends)
mod common;
use common::{run_php, run_php_expect_error};

#[test]
fn test_extends_basic() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "Rex says Woof"
    );
}

#[test]
fn test_extends_inherits_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public function hello() {
        return "Hello";
    }
}
class Child extends Base {}
$c = new Child();
echo $c->hello();
"#
        ),
        "Hello"
    );
}

#[test]
fn static_self_and_parent_calls_use_lexical_class_scope() {
    assert_eq!(
        run_php(
            r#"<?php
namespace StaticScope;

class Base {
    public static function value(): int { return 1; }
}

class Child extends Base {
    public static function own(): int { return 2; }
    public static function calls(): int {
        return self::own() + self::value() + parent::value();
    }
}

echo Child::calls();
"#
        ),
        "4"
    );
}

#[test]
fn late_static_calls_follow_and_rekey_the_called_class() {
    assert_eq!(
        run_php(
            r#"<?php
class LateRoot {
    public static function value(): string { return "R"; }
    public static function dispatch(): string { return static::value(); }
    public function instanceDispatch(): string { return static::value(); }
}
class LateLeft extends LateRoot {
    public static function value(): string { return "L"; }
}
class LateRight extends LateRoot {
    public static function value(): string { return "X"; }
}

echo LateRoot::dispatch();
echo LateLeft::dispatch();
echo LateRight::dispatch();
echo LateLeft::dispatch();
$right = new LateRight();
echo $right->instanceDispatch();
"#
        ),
        "RLXLX"
    );
}

#[test]
fn late_static_properties_follow_and_rekey_the_called_class() {
    assert_eq!(
        run_php(
            r#"<?php
namespace LateProperties;

class Root {
    public static $value = "R";
    public static function late(): string { return static::$value; }
    public static function lexical(): string { return self::$value; }
    public function instanceLate(): string { return static::$value; }
}
class Left extends Root {
    public static $value = "L";
}
class Right extends Root {
    public static $value = "X";
}

echo Root::late();
echo Left::late();
echo Right::late();
echo Left::late();
echo Right::lexical();
$right = new Right();
echo $right->instanceLate();
"#
        ),
        "RLXLRX"
    );
}

#[test]
fn closures_capture_late_static_property_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class PropertyClosureRoot {
    public static $value = "R";
    public static function make() {
        return fn(): string => static::$value;
    }
}
class PropertyClosureChild extends PropertyClosureRoot {
    public static $value = "C";
}
$root = PropertyClosureRoot::make();
$child = PropertyClosureChild::make();
echo $root() . $child() . $root();
"#
        ),
        "RCR"
    );
}

#[test]
fn late_static_property_visibility_uses_lexical_caller_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class ProtectedPropertyRoot {
    protected static $value = "root";
    public static function read(): string { return static::$value; }
}
class ProtectedPropertyChild extends ProtectedPropertyRoot {
    protected static $value = "child";
}
echo ProtectedPropertyRoot::read() . ":" . ProtectedPropertyChild::read();
"#
        ),
        "root:child"
    );

    let error = run_php_expect_error(
        r#"<?php
class PrivatePropertyRoot {
    private static $value = "root";
    public static function read(): string { return static::$value; }
}
class PrivatePropertyChild extends PrivatePropertyRoot {
    private static $value = "child";
}
PrivatePropertyChild::read();
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Cannot access private property PrivatePropertyChild::$value"),
        "{rendered:?}"
    );
}

#[test]
fn mutable_static_properties_share_only_inherited_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class MutableStaticRoot {
    public static $value = "root";
}
class MutableStaticInherited extends MutableStaticRoot {}
class MutableStaticRedeclared extends MutableStaticRoot {
    public static $value = "redeclared";
}

MutableStaticInherited::$value = "shared";
echo MutableStaticRoot::$value . ":" . MutableStaticInherited::$value . ":";
MutableStaticRedeclared::$value = "separate";
echo MutableStaticRoot::$value . ":" . MutableStaticRedeclared::$value;
"#,
        ),
        "shared:shared:shared:separate"
    );
}

#[test]
fn late_static_property_assignment_rekeys_and_self_remains_lexical() {
    assert_eq!(
        run_php(
            r#"<?php
class MutableLateRoot {
    public static $value = "root";
    public static function lateWrite(string $value): void { static::$value = $value; }
    public static function selfWrite(string $value): void { self::$value = $value; }
    public function instanceWrite(string $value): void { static::$value = $value; }
}
class MutableLateLeft extends MutableLateRoot { public static $value = "left"; }
class MutableLateRight extends MutableLateRoot { public static $value = "right"; }

MutableLateRoot::lateWrite("R");
MutableLateLeft::lateWrite("L");
MutableLateRight::lateWrite("X");
MutableLateLeft::lateWrite("L2");
MutableLateRight::selfWrite("ROOT");
$right = new MutableLateRight();
$right->instanceWrite("X2");
echo MutableLateRoot::$value . ":" . MutableLateLeft::$value . ":" . MutableLateRight::$value;
"#,
        ),
        "ROOT:L2:X2"
    );
}

#[test]
fn static_property_assignment_enforces_visibility_and_declared_existence() {
    let private = run_php_expect_error(
        r#"<?php
class PrivateMutableStatic { private static $value = 1; }
PrivateMutableStatic::$value = 2;
"#,
    );
    let rendered = format!("{private:?}");
    assert!(
        rendered.contains("Cannot access private property PrivateMutableStatic::$value"),
        "{rendered:?}"
    );

    let missing = run_php_expect_error(
        r#"<?php
class MissingMutableStatic {}
MissingMutableStatic::$value = 2;
"#,
    );
    let rendered = format!("{missing:?}");
    assert!(
        rendered.contains("Access to undeclared static property MissingMutableStatic::$value"),
        "{rendered:?}"
    );
}

#[test]
fn mutable_static_property_compound_assignments_read_then_write_canonical_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class CompoundStaticRoot {
    public static $number = 1;
    public static $text = "a";
    public static function update(): void {
        static::$number += 4;
        self::$text .= "b";
    }
}
class CompoundStaticChild extends CompoundStaticRoot {}

CompoundStaticChild::update();
CompoundStaticRoot::$number *= 3;
CompoundStaticChild::$text .= "c";
echo CompoundStaticRoot::$number . ":" . CompoundStaticChild::$number . ":";
echo CompoundStaticRoot::$text . ":" . CompoundStaticChild::$text;
"#,
        ),
        "15:15:abc:abc"
    );
}

#[test]
fn closures_capture_the_late_called_class_at_creation() {
    assert_eq!(
        run_php(
            r#"<?php
class ClosureRoot {
    public static function value(): string { return "R"; }
    public static function makeClosure() {
        return function(): string { return static::value(); };
    }
    public static function makeArrow() {
        return fn(): string => static::value();
    }
    public function makeInstanceClosure() {
        return function(): string { return static::value(); };
    }
}
class ClosureLeft extends ClosureRoot {
    public static function value(): string { return "L"; }
}
class ClosureRight extends ClosureRoot {
    public static function value(): string { return "X"; }
}

$root = ClosureRoot::makeClosure();
$left = ClosureLeft::makeClosure();
$arrow = ClosureLeft::makeArrow();
$rightObject = new ClosureRight();
$instance = $rightObject->makeInstanceClosure();
echo $root();
echo $left();
echo $root();
echo $arrow();
echo $instance();
"#
        ),
        "RLRLX"
    );
}

#[test]
fn late_static_scope_preserves_compact_heap_cleanup() {
    assert_eq!(
        run_php(
            r#"<?php
class CompactLateRoot {
    public static function value(): string { return "root"; }
    public static function dispatch(): string {
        $prefix = "value:";
        $parts = [$prefix, static::value()];
        return $parts[0] . $parts[1];
    }
}
class CompactLateChild extends CompactLateRoot {
    public static function value(): string { return "child"; }
}
echo CompactLateRoot::dispatch() . ":" . CompactLateChild::dispatch();
"#
        ),
        "value:root:value:child"
    );
}

#[test]
fn wide_late_static_frame_uses_the_sparse_scope_fallback() {
    assert_eq!(
        run_php(
            r#"<?php
class WideLateRoot {
    public static function value(): string { return "root"; }
    public static function dispatch(): string {
        $v00 = 0; $v01 = 1; $v02 = 2; $v03 = 3; $v04 = 4;
        $v05 = 5; $v06 = 6; $v07 = 7; $v08 = 8; $v09 = 9;
        $v10 = 10; $v11 = 11; $v12 = 12; $v13 = 13; $v14 = 14;
        $v15 = 15; $v16 = 16; $v17 = 17; $v18 = 18; $v19 = 19;
        $v20 = 20; $v21 = 21; $v22 = 22; $v23 = 23; $v24 = 24;
        $v25 = 25; $v26 = 26; $v27 = 27; $v28 = 28; $v29 = 29;
        $v30 = 30; $v31 = 31; $v32 = 32; $v33 = 33;
        return static::value() . ($v00 + $v33);
    }
}
class WideLateChild extends WideLateRoot {
    public static function value(): string { return "child"; }
}
echo WideLateRoot::dispatch() . ":";
echo WideLateChild::dispatch() . ":";
echo WideLateRoot::dispatch();
"#
        ),
        "root33:child33:root33"
    );
}

#[test]
fn test_extends_inherits_property_default() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public $x = 42;
}
class Child extends Base {}
$c = new Child();
echo $c->x;
"#
        ),
        "42"
    );
}

#[test]
fn test_extends_child_overrides_method() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "base child"
    );
}

#[test]
fn test_extends_child_adds_property() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public $x = 1;
}
class Child extends Base {
    public $y = 2;
}
$c = new Child();
echo $c->x . " " . $c->y;
"#
        ),
        "1 2"
    );
}

#[test]
fn test_extends_constructor_inherited() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}
class Child extends Base {}
$c = new Child("test");
echo $c->name;
"#
        ),
        "test"
    );
}

#[test]
fn test_extends_constructor_overridden() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "1 2"
    );
}

#[test]
fn test_extends_three_levels() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "AAC"
    );
}

#[test]
fn test_extends_method_uses_this() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "Hi PHP"
    );
}

#[test]
fn test_extends_grandchild_inherits_grandparent_method() {
    // Regression: transitive inheritance must work across 3+ levels
    assert_eq!(
        run_php(
            r#"<?php
class A {
    public function foo() { return "A"; }
}
class B extends A {}
class C extends B {}
$c = new C();
echo $c->foo();
"#
        ),
        "A"
    );
}

#[test]
fn test_extends_grandchild_inherits_constructor() {
    // Regression: constructor must be inherited transitively
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "42"
    );
}
