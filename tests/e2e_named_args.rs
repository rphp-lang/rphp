/// Tests for named arguments (PHP 8 style)
mod common;
use common::run_php;

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

// ── P1 regression: by-ref named arguments ──

#[test]
fn test_named_args_by_ref() {
    assert_eq!(
        run_php(
            r#"<?php
function inc(&$x) { $x = $x + 1; }
$a = 1;
inc(x: $a);
echo $a;
"#
        ),
        "2"
    );
}

#[test]
fn test_named_args_by_ref_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Counter {
    public function bump(&$val) { $val++; }
}
$c = new Counter();
$x = 10;
$c->bump(val: $x);
echo $x;
"#
        ),
        "11"
    );
}

// ── P1 regression: named args skip required param ──

#[test]
fn test_named_args_skip_required_fatal() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
function f($a, $b) { echo $a; }
f(b: 2);
"#,
        );
    });
    assert!(result.is_err());
}

#[test]
fn test_named_args_skip_required_constructor() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
class C {
    public function __construct($a, $b) {}
}
new C(b: 2);
"#,
        );
    });
    assert!(result.is_err());
}

// ── P1 regression: named args with internal functions ──

#[test]
fn test_named_args_strlen_internal() {
    assert_eq!(
        run_php(
            r#"<?php
echo strlen(string: "abc");
"#
        ),
        "3"
    );
}

#[test]
fn test_named_args_substr_internal() {
    assert_eq!(
        run_php(
            r#"<?php
echo substr(string: "abcdef", offset: 2, length: 3);
"#
        ),
        "cde"
    );
}

#[test]
fn test_named_args_exception_constructor() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Exception(message: "test error");
echo $e->getMessage();
"#
        ),
        "test error"
    );
}

#[test]
fn test_named_args_str_replace_internal() {
    assert_eq!(
        run_php(
            r#"<?php
echo str_replace(search: "world", replace: "PHP", subject: "hello world");
"#
        ),
        "hello PHP"
    );
}

#[test]
fn test_named_args_in_array_internal() {
    assert_eq!(
        run_php(
            r#"<?php
echo in_array(needle: 3, haystack: [1,2,3]) ? "yes" : "no";
"#
        ),
        "yes"
    );
}

// ── P1 regression: duplicate/overwrite detection ──

#[test]
fn test_named_args_duplicate_named() {
    assert_eq!(
        run_php(
            r#"<?php
function f($a) { echo $a; }
try {
    f(a: 1, a: 2);
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

#[test]
fn test_named_args_positional_then_named_overwrite() {
    assert_eq!(
        run_php(
            r#"<?php
function f($a, $b = 10, $c = 20) { echo "$a:$b:$c"; }
try {
    f(1, a: 2);
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── P1 regression: keyword-like parameter names ──

#[test]
fn test_named_args_keyword_param_array() {
    assert_eq!(
        run_php(
            r#"<?php
function f($array) { echo $array; }
f(array: 42);
"#
        ),
        "42"
    );
}

#[test]
fn test_named_args_keyword_param_string() {
    assert_eq!(
        run_php(
            r#"<?php
echo strlen(string: "hello");
"#
        ),
        "5"
    );
}

#[test]
fn test_named_args_keyword_param_match() {
    assert_eq!(
        run_php(
            r#"<?php
function f($match) { echo $match; }
f(match: "ok");
"#
        ),
        "ok"
    );
}

// ── P1 regression: variadic named arguments ──

#[test]
fn test_named_args_variadic_extra() {
    assert_eq!(
        run_php(
            r#"<?php
function f($a, ...$rest) {
    echo $a . ":" . (isset($rest["b"]) ? $rest["b"] : "no");
}
f(a: 1, b: 2);
"#
        ),
        "1:2"
    );
}

#[test]
fn test_named_args_variadic_only() {
    assert_eq!(
        run_php(
            r#"<?php
function f(...$args) {
    echo isset($args["x"]) ? $args["x"] : "no";
    echo " ";
    echo isset($args["y"]) ? $args["y"] : "no";
}
f(x: 10, y: 20);
"#
        ),
        "10 20"
    );
}

#[test]
fn test_named_args_variadic_mixed() {
    assert_eq!(
        run_php(
            r#"<?php
function f($a, ...$rest) {
    echo $a . ":" . count($rest);
}
f(1, b: 2, c: 3);
"#
        ),
        "1:2"
    );
}

// ── P2 regression: count() param name ──

#[test]
fn test_named_args_count_param_name() {
    assert_eq!(
        run_php(
            r#"<?php
echo count(value: [1,2,3]);
"#
        ),
        "3"
    );
}

// ── P1 regression: variadic leak after caught TypeError ──

#[test]
fn test_named_args_variadic_leak_after_type_error() {
    assert_eq!(
        run_php(
            r#"<?php
function f(int $a, ...$rest) { echo count($rest); }
try { f(a: "x", b: 2); } catch (TypeError $e) {}
f(a: 1);
"#
        ),
        "0"
    );
}

// ── P1 regression: duplicate named extras on variadics ──

#[test]
fn test_named_args_variadic_duplicate_extra() {
    assert_eq!(
        run_php(
            r#"<?php
function f(...$args) { var_dump($args); }
try {
    f(x: 1, x: 2);
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── P1 regression: named arg targeting variadic param name ──

#[test]
fn test_named_args_variadic_param_name_direct() {
    assert_eq!(
        run_php(
            r#"<?php
function f($a, ...$rest) {
    echo isset($rest["rest"]) ? $rest["rest"] : "no";
}
f(a: 1, rest: 2);
"#
        ),
        "2"
    );
}

#[test]
fn test_named_args_variadic_only_param_name() {
    assert_eq!(
        run_php(
            r#"<?php
function f(...$rest) {
    echo isset($rest["rest"]) ? $rest["rest"] : "no";
}
f(rest: 1);
"#
        ),
        "1"
    );
}

// ── P2 regression: finally keyword as named arg label ──

#[test]
fn test_named_args_keyword_finally() {
    assert_eq!(
        run_php(
            r#"<?php
function f($finally) { echo $finally; }
f(finally: 42);
"#
        ),
        "42"
    );
}

// ── P1 regression: internal variadic rejects unknown named params ──

#[test]
fn test_named_args_internal_variadic_rejects_named() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo sprintf(format: "%s %s", first: "a", second: "b");
} catch (Error $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── P1 regression: named variadic extras must pass type hint ──

#[test]
fn test_named_args_typed_variadic_pass() {
    assert_eq!(
        run_php(
            r#"<?php
function f(string ...$rest) { echo count($rest); }
f(a: "hello", b: "world");
"#
        ),
        "2"
    );
}

#[test]
fn test_named_args_typed_variadic_fail() {
    assert_eq!(
        run_php(
            r#"<?php
function f(string ...$rest) { echo "ok"; }
try {
    f(rest: 1);
} catch (TypeError $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}

// ── P2 regression: fn and use keyword labels ──

#[test]
fn test_named_args_keyword_fn() {
    assert_eq!(
        run_php(
            r#"<?php
function f($fn) { echo $fn; }
f(fn: 42);
"#
        ),
        "42"
    );
}

#[test]
fn test_named_args_keyword_use() {
    assert_eq!(
        run_php(
            r#"<?php
function f($use) { echo $use; }
f(use: 42);
"#
        ),
        "42"
    );
}

// ── Catchable named-arg errors ──

#[test]
fn test_named_args_unknown_param_catchable_message() {
    assert_eq!(
        run_php(
            r#"<?php
function foo(int $a) { echo $a; }
try {
    foo(b: 42);
} catch (Error $e) {
    echo $e->getMessage();
}
"#
        ),
        "Unknown named parameter $b"
    );
}

#[test]
fn test_named_args_overwrite_catchable_message() {
    assert_eq!(
        run_php(
            r#"<?php
function f($a) { echo $a; }
try {
    f(1, a: 2);
} catch (Error $e) {
    echo $e->getMessage();
}
"#
        ),
        "Named parameter $a overwrites previous argument"
    );
}

#[test]
fn test_named_args_catchable_recovery() {
    assert_eq!(
        run_php(
            r#"<?php
function f($a) { echo $a; }
try {
    f(b: 42);
} catch (Error $e) {
    echo "caught ";
}
f(a: 99);
"#
        ),
        "caught 99"
    );
}
