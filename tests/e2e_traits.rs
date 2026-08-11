mod common;
use common::run_php;

// ─── Basic trait usage ────────────────────────────────────────────

#[test]
fn trait_basic_method() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function hello() {
        echo "Hello from trait\n";
    }
}
class MyClass {
    use Greet;
}
$obj = new MyClass();
$obj->hello();
"#,
    );
    assert_eq!(out, "Hello from trait\n");
}

#[test]
fn trait_method_with_params() {
    let out = run_php(
        r#"<?php
trait MathTrait {
    public function add($a, $b) {
        return $a + $b;
    }
}
class Calc {
    use MathTrait;
}
$c = new Calc();
echo $c->add(3, 7);
"#,
    );
    assert_eq!(out, "10");
}

#[test]
fn trait_multiple_methods() {
    let out = run_php(
        r#"<?php
trait Logger {
    public function log($msg) {
        echo "LOG: " . $msg . "\n";
    }
    public function warn($msg) {
        echo "WARN: " . $msg . "\n";
    }
}
class App {
    use Logger;
}
$app = new App();
$app->log("started");
$app->warn("low memory");
"#,
    );
    assert_eq!(out, "LOG: started\nWARN: low memory\n");
}

// ─── Multiple traits ──────────────────────────────────────────────

#[test]
fn trait_use_multiple() {
    let out = run_php(
        r#"<?php
trait A {
    public function fromA() { echo "A"; }
}
trait B {
    public function fromB() { echo "B"; }
}
class C {
    use A, B;
}
$c = new C();
$c->fromA();
$c->fromB();
"#,
    );
    assert_eq!(out, "AB");
}

// ─── Trait with properties ────────────────────────────────────────

#[test]
fn trait_with_property() {
    let out = run_php(
        r#"<?php
trait HasName {
    public $name = "default";
}
class User {
    use HasName;
}
$u = new User();
echo $u->name . "\n";
$u->name = "Alice";
echo $u->name;
"#,
    );
    assert_eq!(out, "default\nAlice");
}

// ─── Class method overrides trait ─────────────────────────────────

#[test]
fn trait_class_overrides_method() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function hello() {
        echo "trait";
    }
}
class MyClass {
    use Greet;
    public function hello() {
        echo "class";
    }
}
$obj = new MyClass();
$obj->hello();
"#,
    );
    assert_eq!(out, "class");
}

// ─── Trait + inheritance ──────────────────────────────────────────

#[test]
fn trait_with_inheritance() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function hello() {
        echo "trait hello\n";
    }
}
class Base {
    public function base_method() {
        echo "base\n";
    }
}
class Child extends Base {
    use Greet;
}
$c = new Child();
$c->base_method();
$c->hello();
"#,
    );
    assert_eq!(out, "base\ntrait hello\n");
}

#[test]
fn trait_overrides_parent_method() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function hello() {
        echo "from trait";
    }
}
class Base {
    public function hello() {
        echo "from base";
    }
}
class Child extends Base {
    use Greet;
}
$c = new Child();
$c->hello();
"#,
    );
    assert_eq!(out, "from trait");
}

// ─── Trait satisfies interface ────────────────────────────────────

#[test]
fn trait_satisfies_interface() {
    let out = run_php(
        r#"<?php
interface Loggable {
    public function log($msg);
}
trait LogTrait {
    public function log($msg) {
        echo $msg;
    }
}
class App implements Loggable {
    use LogTrait;
}
$app = new App();
$app->log("works!");
"#,
    );
    assert_eq!(out, "works!");
}

// ─── Trait with $this ─────────────────────────────────────────────

#[test]
fn trait_method_accesses_this() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function greet() {
        echo "Hello, " . $this->name;
    }
}
class Person {
    use Greet;
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}
$p = new Person("Alice");
$p->greet();
"#,
    );
    assert_eq!(out, "Hello, Alice");
}

// ─── Static trait methods ─────────────────────────────────────────

#[test]
fn trait_static_method() {
    let out = run_php(
        r#"<?php
trait Counter {
    public static function count_to($n) {
        $i = 1;
        while ($i <= $n) {
            echo $i;
            $i = $i + 1;
        }
    }
}
class App {
    use Counter;
}
App::count_to(3);
"#,
    );
    assert_eq!(out, "123");
}

#[test]
fn trait_static_pseudo_calls_resolve_for_each_consuming_class() {
    let out = run_php(
        r#"<?php
trait CallsScope {
    public static function selfValue(): string { return self::value(); }
    public static function parentValue(): string { return parent::value(); }
}
class FirstBase {
    public static function value(): string { return "first-base"; }
}
class First extends FirstBase {
    use CallsScope;
    public static function value(): string { return "first"; }
}
class SecondBase {
    public static function value(): string { return "second-base"; }
}
class Second extends SecondBase {
    use CallsScope;
    public static function value(): string { return "second"; }
}
echo First::selfValue() . ":" . First::parentValue() . ":";
echo Second::selfValue() . ":" . Second::parentValue() . ":";
echo First::selfValue() . ":" . First::parentValue();
"#,
    );
    assert_eq!(out, "first:first-base:second:second-base:first:first-base");
}

#[test]
fn trait_late_static_calls_follow_each_consuming_class() {
    let out = run_php(
        r#"<?php
trait LateTraitCall {
    public static function dispatch(): string { return static::value(); }
}
class LateTraitFirst {
    use LateTraitCall;
    public static function value(): string { return "first"; }
}
class LateTraitSecond {
    use LateTraitCall;
    public static function value(): string { return "second"; }
}
echo LateTraitFirst::dispatch() . ":";
echo LateTraitSecond::dispatch() . ":";
echo LateTraitFirst::dispatch();
"#,
    );
    assert_eq!(out, "first:second:first");
}

// ─── Trait property collision edge cases ──────────────────────────

#[test]
fn trait_property_same_default_ok() {
    // Two traits with same property, same visibility, same default → ok
    let out = run_php(
        r#"<?php
trait T1 { public $x = 1; }
trait T2 { public $x = 1; }
class C {
    use T1, T2;
}
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(out, "1");
}

#[test]
fn trait_property_different_default_rejected() {
    // Two traits with same property, same visibility, different default → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $x = 1; }
trait T2 { public $x = 2; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible trait property defaults"
    );
}

#[test]
fn trait_property_different_visibility_rejected() {
    // Two traits with same property but different visibility → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $x = 1; }
trait T2 { protected $x = 1; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible trait property visibility"
    );
}

#[test]
fn trait_property_class_overrides_trait() {
    // Class's own property always takes precedence over trait's
    let out = run_php(
        r#"<?php
trait T1 { public $x = 10; }
class C {
    use T1;
    public $x = 99;
}
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(out, "99");
}

#[test]
fn trait_property_string_default_same_ok() {
    // String defaults that are equal → ok
    let out = run_php(
        r#"<?php
trait T1 { public $name = "hello"; }
trait T2 { public $name = "hello"; }
class C {
    use T1, T2;
}
$c = new C();
echo $c->name;
"#,
    );
    assert_eq!(out, "hello");
}

#[test]
fn trait_property_string_default_different_rejected() {
    // String defaults that differ → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $name = "hello"; }
trait T2 { public $name = "world"; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible string defaults"
    );
}

#[test]
fn trait_property_null_vs_value_rejected() {
    // One trait has null default, other has int default → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $x = null; }
trait T2 { public $x = 0; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from null vs value default mismatch"
    );
}

#[test]
fn trait_property_array_default_same_ok() {
    // Two traits with same array default → compatible
    let out = run_php(
        r#"<?php
trait T1 { public $items = [1, 2, 3]; }
trait T2 { public $items = [1, 2, 3]; }
class C {
    use T1, T2;
}
$c = new C();
echo count($c->items);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn trait_property_array_default_different_rejected() {
    // Two traits with different array defaults → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $items = [1, 2]; }
trait T2 { public $items = [1, 3]; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible array defaults"
    );
}

#[test]
fn trait_property_array_different_length_rejected() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $x = [1]; }
trait T2 { public $x = [1, 2]; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from different length array defaults"
    );
}
