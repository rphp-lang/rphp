mod common;
use common::*;

// ============================================================================
// const keyword
// ============================================================================

#[test]
fn test_const_basic() {
    let out = run_php(r#"<?php
const FOO = 42;
echo FOO;
"#);
    assert_eq!(out, "42");
}

#[test]
fn test_const_string() {
    let out = run_php(r#"<?php
const GREETING = "hello";
echo GREETING;
"#);
    assert_eq!(out, "hello");
}

#[test]
fn test_const_in_expression() {
    let out = run_php(r#"<?php
const X = 10;
const Y = 20;
echo X + Y;
"#);
    assert_eq!(out, "30");
}

#[test]
fn test_const_bool_and_null() {
    let out = run_php(r#"<?php
const A = true;
const B = false;
const C = null;
echo A;
echo B;
echo C;
"#);
    assert_eq!(out, "1");
}

#[test]
fn test_const_float() {
    let out = run_php(r#"<?php
const PI = 3.14;
echo PI;
"#);
    assert_eq!(out, "3.14");
}

#[test]
fn test_const_used_in_function() {
    let out = run_php(r#"<?php
const MAX = 100;
function getMax() {
    return MAX;
}
echo getMax();
"#);
    assert_eq!(out, "100");
}

#[test]
fn test_const_in_condition() {
    let out = run_php(r#"<?php
const DEBUG = true;
if (DEBUG) {
    echo "debug on";
} else {
    echo "debug off";
}
"#);
    assert_eq!(out, "debug on");
}

// ============================================================================
// define() function
// ============================================================================

#[test]
fn test_define_basic() {
    let out = run_php(r#"<?php
define("BAR", 99);
echo BAR;
"#);
    assert_eq!(out, "99");
}

#[test]
fn test_define_string_value() {
    let out = run_php(r#"<?php
define("APP_NAME", "MyApp");
echo APP_NAME;
"#);
    assert_eq!(out, "MyApp");
}

#[test]
fn test_defined_true() {
    let out = run_php(r#"<?php
const THING = 1;
if (defined("THING")) {
    echo "yes";
} else {
    echo "no";
}
"#);
    assert_eq!(out, "yes");
}

#[test]
fn test_defined_false() {
    let out = run_php(r#"<?php
if (defined("NOPE")) {
    echo "yes";
} else {
    echo "no";
}
"#);
    assert_eq!(out, "no");
}

#[test]
fn test_constant_function() {
    let out = run_php(r#"<?php
define("KEY", "value123");
echo constant("KEY");
"#);
    assert_eq!(out, "value123");
}

#[test]
fn test_define_and_const_coexist() {
    let out = run_php(r#"<?php
const A = 1;
define("B", 2);
echo A + B;
"#);
    assert_eq!(out, "3");
}

// ============================================================================
// Default parameter values
// ============================================================================

#[test]
fn test_default_param_basic() {
    let out = run_php(r#"<?php
function greet($name = "World") {
    echo "Hello " . $name;
}
greet();
"#);
    assert_eq!(out, "Hello World");
}

#[test]
fn test_default_param_override() {
    let out = run_php(r#"<?php
function greet($name = "World") {
    echo "Hello " . $name;
}
greet("PHP");
"#);
    assert_eq!(out, "Hello PHP");
}

#[test]
fn test_default_param_multiple() {
    let out = run_php(r#"<?php
function add($a, $b = 10, $c = 20) {
    return $a + $b + $c;
}
echo add(1);
echo " ";
echo add(1, 2);
echo " ";
echo add(1, 2, 3);
"#);
    assert_eq!(out, "31 23 6");
}

#[test]
fn test_default_param_null() {
    let out = run_php(r#"<?php
function test($x = null) {
    if ($x === null) {
        echo "null";
    } else {
        echo $x;
    }
}
test();
"#);
    assert_eq!(out, "null");
}

#[test]
fn test_default_param_bool() {
    let out = run_php(r#"<?php
function check($verbose = false) {
    if ($verbose) {
        echo "verbose";
    } else {
        echo "quiet";
    }
}
check();
echo " ";
check(true);
"#);
    assert_eq!(out, "quiet verbose");
}

#[test]
fn test_default_param_integer() {
    let out = run_php(r#"<?php
function repeat($str, $times = 3) {
    $result = "";
    for ($i = 0; $i < $times; $i++) {
        $result .= $str;
    }
    echo $result;
}
repeat("a");
echo " ";
repeat("b", 2);
"#);
    assert_eq!(out, "aaa bb");
}

#[test]
fn test_default_param_in_class_method() {
    let out = run_php(r#"<?php
class Greeter {
    public function hello($name = "World") {
        echo "Hi " . $name;
    }
}
$g = new Greeter();
$g->hello();
echo " ";
$g->hello("PHP");
"#);
    assert_eq!(out, "Hi World Hi PHP");
}

#[test]
fn test_default_param_in_closure() {
    let out = run_php(r#"<?php
$add = function($a, $b = 5) {
    return $a + $b;
};
echo $add(10);
echo " ";
echo $add(10, 20);
"#);
    assert_eq!(out, "15 30");
}

#[test]
fn test_default_param_expression() {
    let out = run_php(r#"<?php
function test($x = 2 + 3) {
    echo $x;
}
test();
"#);
    assert_eq!(out, "5");
}

#[test]
fn test_default_param_string_concat() {
    let out = run_php(r#"<?php
function test($prefix = "Hello" . " " . "World") {
    echo $prefix;
}
test();
"#);
    assert_eq!(out, "Hello World");
}

// ============================================================================
// Edge cases
// ============================================================================

#[test]
fn test_const_with_function_call() {
    let out = run_php(r#"<?php
const SEPARATOR = "-";
function join_parts($a, $b) {
    return $a . SEPARATOR . $b;
}
echo join_parts("hello", "world");
"#);
    assert_eq!(out, "hello-world");
}

#[test]
fn test_many_defaults() {
    let out = run_php(r#"<?php
function config($host = "localhost", $port = 3306, $db = "test") {
    echo $host . ":" . $port . "/" . $db;
}
config();
echo "|";
config("prod");
echo "|";
config("prod", 5432);
echo "|";
config("prod", 5432, "mydb");
"#);
    assert_eq!(out, "localhost:3306/test|prod:3306/test|prod:5432/test|prod:5432/mydb");
}

#[test]
fn test_define_with_variable() {
    let out = run_php(r#"<?php
$val = 42;
define("DYNAMIC", $val);
echo DYNAMIC;
"#);
    assert_eq!(out, "42");
}

// ============================================================================
// Regression: P1 — side-effect defaults must NOT run when arg is passed
// ============================================================================

#[test]
fn test_default_side_effect_skipped_when_arg_passed() {
    // The default calls a function with a side effect (echo).
    // When arg IS passed, the default must NOT be evaluated.
    let out = run_php(r#"<?php
function side() {
    echo "SIDE";
    return 99;
}
function test($x = side()) {
    echo $x;
}
test(5);
"#);
    // Must output "5" only — "SIDE" must NOT appear
    assert_eq!(out, "5");
}

#[test]
fn test_default_side_effect_runs_when_arg_omitted() {
    // When arg is NOT passed, the default expression IS evaluated
    let out = run_php(r#"<?php
function side() {
    echo "SIDE";
    return 99;
}
function test($x = side()) {
    echo $x;
}
test();
"#);
    assert_eq!(out, "SIDE99");
}

#[test]
fn test_default_side_effect_mixed() {
    // Multiple calls: first with arg (no side effect), second without (side effect)
    let out = run_php(r#"<?php
$count = 0;
function counter() {
    echo "C";
    return 1;
}
function test($a, $b = counter()) {
    echo $a . $b;
}
test(1, 2);
echo "|";
test(3);
"#);
    assert_eq!(out, "12|C31");
}

// ============================================================================
// Regression: P2 — define() with non-string name uses string coercion
// ============================================================================

#[test]
fn test_define_integer_name_coerces_to_string() {
    // PHP coerces integer name to string "123"
    let out = run_php(r#"<?php
define("123", "val");
echo defined("123") ? "yes" : "no";
echo " ";
echo constant("123");
"#);
    assert_eq!(out, "yes val");
}

#[test]
fn test_define_returns_false_for_empty_name() {
    let out = run_php(r#"<?php
$result = define("", "val");
echo $result ? "true" : "false";
"#);
    assert_eq!(out, "false");
}

// ============================================================================
// Regression: P2 — constant in class property default must error, not silently null
// ============================================================================

#[test]
fn test_const_in_class_property_default_is_compile_error() {
    // Named constants cannot be resolved at compile time for property defaults.
    // This must be a compile error, NOT a silent null.
    let tokens = rphp::lexer::Lexer::new(r#"<?php
const FOO = 42;
class C { public $x = FOO; }
"#).tokenize().unwrap();
    let stmts = rphp::parser::Parser::new(tokens).parse().unwrap();
    let result = rphp::compiler::compile::Compiler::new().compile(&stmts);
    let err = result.err().expect("Expected compile error for constant in property default");
    assert!(err.contains("not a compile-time constant"), "Error should mention compile-time constant, got: {}", err);
}

// ============================================================================
// Regression: P2 — constant as function default (runtime eval — works fine)
// ============================================================================

#[test]
fn test_const_used_as_function_default() {
    // Constants work fine as function default expressions (evaluated at runtime)
    let out = run_php(r#"<?php
const DEFAULT_PORT = 8080;
function connect($port = DEFAULT_PORT) {
    echo $port;
}
connect();
echo " ";
connect(3000);
"#);
    assert_eq!(out, "8080 3000");
}
