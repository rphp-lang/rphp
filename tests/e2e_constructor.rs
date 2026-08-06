/// Tests for __construct() constructor
mod common;
use common::run_php;
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;

fn compile_constructor_source(source: &str) -> rphp::compiler::compile::CompileResult {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    Compiler::new().compile(&statements).unwrap()
}

#[test]
fn test_constructor_basic() {
    assert_eq!(
        run_php(
            r#"<?php
class Dog {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}
$d = new Dog("Rex");
echo $d->name;
"#
        ),
        "Rex"
    );
}

#[test]
fn test_constructor_multiple_args() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "3,4"
    );
}

#[test]
fn test_constructor_with_method() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "Hello World"
    );
}

#[test]
fn test_constructor_no_args() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "2"
    );
}

#[test]
fn test_constructor_default_overridden() {
    assert_eq!(
        run_php(
            r#"<?php
class Config {
    public $timeout = 30;
    public function __construct($t) {
        $this->timeout = $t;
    }
}
$c = new Config(60);
echo $c->timeout;
"#
        ),
        "60"
    );
}

#[test]
fn test_no_constructor_no_args() {
    // Class without constructor — new still works
    assert_eq!(
        run_php(
            r#"<?php
class Empty2 {}
$e = new Empty2();
echo "ok";
"#
        ),
        "ok"
    );
}

#[test]
fn test_multiple_objects_different_constructor_args() {
    assert_eq!(
        run_php(
            r#"<?php
class Box {
    public $value;
    public function __construct($v) {
        $this->value = $v;
    }
}
$a = new Box(10);
$b = new Box(20);
echo $a->value . " " . $b->value;
"#
        ),
        "10 20"
    );
}

#[test]
fn test_no_constructor_with_args_silently_ignored() {
    // PHP evaluates arg expressions (side effects run) but ignores values
    // when class has no __construct
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
function side() { echo "S"; return 1; }
$f = new Foo(side());
echo "X";
"#
        ),
        "SX"
    );
}

#[test]
fn test_no_constructor_negative_cache_keeps_argument_side_effects() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainBox { public $value = 7; }
$sum = 0;
for ($i = 0; $i < 5; $i++) {
    $box = new PlainBox($sum = $sum + 1);
}
echo $sum . ':' . $box->value;
"#
        ),
        "5:7"
    );
}

#[test]
fn test_declared_property_constructor_gets_init_plan() {
    let result = compile_constructor_source(
        r#"<?php
class Request {
    public $subtotal;
    public $level;
    public $region;
    public function __construct(int $subtotal, int $level, string $region) {
        $this->subtotal = $subtotal;
        $this->level = $level;
        $this->region = $region;
    }
}
"#,
    );
    let constructor = result.class_defs[0]
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "__construct")
        .map(|(_, _, _, _, function)| function)
        .unwrap();
    let plan = constructor
        .property_init_plan
        .as_deref()
        .expect("declared property constructor init plan");
    assert_eq!(plan.public_args, 3);
    assert_eq!(plan.assignments.len(), 3);
}

#[test]
fn test_constructor_init_plan_preserves_named_type_and_dynamic_fallbacks() {
    assert_eq!(
        run_php(
            r#"<?php
class DeclaredDto {
    public $first;
    public $second;
    public function __construct(int $first, int $second) {
        $this->first = $first;
        $this->second = $second;
    }
}
class DynamicDto {
    public function __construct($value) { $this->value = $value; }
}
$named = new DeclaredDto(second: 4, first: 3);
for ($i = 0; $i < 20; $i++) { $warm = new DeclaredDto($i, $i + 1); }
for ($i = 0; $i < 20; $i++) { $dynamic = new DynamicDto($i); }
echo $named->first . ':' . $named->second . '|' . $dynamic->value . '|';
try {
    new DeclaredDto('bad', 1);
} catch (TypeError $error) {
    echo 'typed';
}
"#
        ),
        "3:4|19|typed"
    );
}
