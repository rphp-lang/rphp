// ── Basic named arguments ──

#[test]
fn test_named_args_basic() {
    assert_eq!(
        run_php(
            r#"<?php
function greet(string $first, string $last) { echo "$first $last"; }
greet(first: "John", last: "Doe");
"#
        ),
        "John Doe"
    );
}

#[test]
fn test_named_args_reorder() {
    assert_eq!(
        run_php(
            r#"<?php
function greet(string $first, string $last) { echo "$first $last"; }
greet(last: "Doe", first: "John");
"#
        ),
        "John Doe"
    );
}

#[test]
fn test_named_args_skip_optional() {
    assert_eq!(
        run_php(
            r#"<?php
function show(int $a, int $b = 10, int $c = 20) { echo "$a $b $c"; }
show(1, c: 99);
"#
        ),
        "1 10 99"
    );
}

#[test]
fn test_named_args_all_named() {
    assert_eq!(
        run_php(
            r#"<?php
function add(int $x, int $y) { return $x + $y; }
echo add(y: 3, x: 7);
"#
        ),
        "10"
    );
}

#[test]
fn test_named_args_mixed_positional_and_named() {
    assert_eq!(
        run_php(
            r#"<?php
function info(string $name, int $age, string $city) {
    echo "$name $age $city";
}
info("Alice", city: "Prague", age: 30);
"#
        ),
        "Alice 30 Prague"
    );
}

// ── Named args with methods ──

#[test]
fn test_named_args_method_call() {
    assert_eq!(
        run_php(
            r#"<?php
class Calc {
    public function add(int $a, int $b) { return $a + $b; }
}
$c = new Calc();
echo $c->add(b: 5, a: 3);
"#
        ),
        "8"
    );
}

#[test]
fn test_named_args_constructor() {
    assert_eq!(
        run_php(
            r#"<?php
class Point {
    public $x;
    public $y;
    public function __construct(int $x, int $y) {
        $this->x = $x;
        $this->y = $y;
    }
}
$p = new Point(y: 20, x: 10);
echo $p->x . " " . $p->y;
"#
        ),
        "10 20"
    );
}

// ── Named args with closures ──

#[test]
fn test_named_args_closure() {
    assert_eq!(
        run_php(
            r#"<?php
$sub = function(int $a, int $b) { return $a - $b; };
echo $sub(b: 3, a: 10);
"#
        ),
        "7"
    );
}

// ── Named args with default values ──

#[test]
fn test_named_args_with_defaults() {
    assert_eq!(
        run_php(
            r#"<?php
function connect(string $host = "localhost", int $port = 3306, string $db = "test") {
    echo "$host:$port/$db";
}
connect(db: "mydb", port: 8080);
"#
        ),
        "localhost:8080/mydb"
    );
}

#[test]
fn test_named_args_override_one_default() {
    assert_eq!(
        run_php(
            r#"<?php
function greet(string $greeting = "Hello", string $name = "World") {
    echo "$greeting $name";
}
greet(name: "PHP");
"#
        ),
        "Hello PHP"
    );
}

// ── Error cases ──

#[test]
fn test_named_args_unknown_param() {
    assert_eq!(
        run_php(
            r#"<?php
function foo(int $a) { echo $a; }
try {
    foo(b: 42);
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── Named args with type hints ──

#[test]
fn test_named_args_type_hint_pass() {
    assert_eq!(
        run_php(
            r#"<?php
function typed(int $x, string $s) { echo "$x $s"; }
typed(s: "hello", x: 42);
"#
        ),
        "42 hello"
    );
}

#[test]
fn test_named_args_type_hint_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function typed(int $x, string $s) { echo "$x $s"; }
try {
    typed(s: 42, x: "hello");
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── Static method with named args ──

#[test]
fn test_named_args_static_call() {
    assert_eq!(
        run_php(
            r#"<?php
class Math {
    public static function divide(int $a, int $b) { return $a / $b; }
}
echo Math::divide(b: 2, a: 10);
"#
        ),
        "5"
    );
}
