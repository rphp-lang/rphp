/// E2E tests: variadic parameters (...$args)
mod common;
use common::run_php;

// ============================================================================
// Basic variadic
// ============================================================================

#[test]
fn test_variadic_basic() {
    assert_eq!(run_php(r#"<?php
function sum(...$nums) {
    $total = 0;
    foreach ($nums as $n) {
        $total += $n;
    }
    return $total;
}
echo sum(1, 2, 3);
"#), "6");
}

#[test]
fn test_variadic_no_args() {
    assert_eq!(run_php(r#"<?php
function collect(...$items) {
    echo count($items);
}
collect();
"#), "0");
}

#[test]
fn test_variadic_single_arg() {
    assert_eq!(run_php(r#"<?php
function first(...$args) {
    echo $args[0];
}
first(42);
"#), "42");
}

#[test]
fn test_variadic_with_fixed_params() {
    assert_eq!(run_php(r#"<?php
function greet($greeting, ...$names) {
    foreach ($names as $name) {
        echo $greeting . " " . $name . "\n";
    }
}
greet("Hello", "Alice", "Bob");
"#), "Hello Alice\nHello Bob\n");
}

#[test]
fn test_variadic_with_fixed_and_default() {
    assert_eq!(run_php(r#"<?php
function my_log($level = "INFO", ...$messages) {
    foreach ($messages as $msg) {
        echo "[" . $level . "] " . $msg . "\n";
    }
}
my_log("WARN", "disk full", "retry");
"#), "[WARN] disk full\n[WARN] retry\n");
}

#[test]
fn test_variadic_count() {
    assert_eq!(run_php(r#"<?php
function nargs(...$args) {
    return count($args);
}
echo nargs();
echo " ";
echo nargs(1);
echo " ";
echo nargs(1, 2, 3, 4, 5);
"#), "0 1 5");
}

#[test]
fn test_variadic_mixed_types() {
    assert_eq!(run_php(r#"<?php
function dump(...$args) {
    foreach ($args as $a) {
        echo gettype($a) . " ";
    }
}
dump(1, "hello", true, null, 3.14);
"#), "integer string boolean NULL double ");
}

#[test]
fn test_variadic_pass_to_another_function() {
    assert_eq!(run_php(r#"<?php
function add($a, $b) {
    return $a + $b;
}
function apply_add(...$args) {
    return add($args[0], $args[1]);
}
echo apply_add(3, 7);
"#), "10");
}

#[test]
fn test_variadic_in_method() {
    assert_eq!(run_php(r#"<?php
class Logger {
    public function log($prefix, ...$messages) {
        foreach ($messages as $msg) {
            echo $prefix . ": " . $msg . "\n";
        }
    }
}
$l = new Logger();
$l->log("APP", "started", "ready");
"#), "APP: started\nAPP: ready\n");
}

#[test]
fn test_variadic_in_closure() {
    assert_eq!(run_php(r#"<?php
$sum = function(...$nums) {
    $t = 0;
    foreach ($nums as $n) { $t += $n; }
    return $t;
};
echo $sum(10, 20, 30);
"#), "60");
}

#[test]
fn test_variadic_in_arrow_function() {
    assert_eq!(run_php(r#"<?php
$count = fn(...$args) => count($args);
echo $count(1, 2, 3);
"#), "3");
}

#[test]
fn test_variadic_is_array() {
    assert_eq!(run_php(r#"<?php
function check(...$args) {
    echo is_array($args) ? "yes" : "no";
}
check(1, 2);
"#), "yes");
}
