mod common;
use common::run_php;

// list() / destructuring tests

#[test]
fn test_list_basic() {
    assert_eq!(run_php(r#"<?php
list($a, $b, $c) = [10, 20, 30];
echo "$a $b $c";
"#), "10 20 30");
}

#[test]
fn test_short_destructuring() {
    assert_eq!(run_php(r#"<?php
[$a, $b] = [1, 2];
echo "$a $b";
"#), "1 2");
}

#[test]
fn test_list_skip_elements() {
    assert_eq!(run_php(r#"<?php
[, $b, , $d] = [1, 2, 3, 4];
echo "$b $d";
"#), "2 4");
}

#[test]
fn test_list_from_function() {
    assert_eq!(run_php(r#"<?php
function pair() { return [42, "hello"]; }
[$num, $str] = pair();
echo "$num $str";
"#), "42 hello");
}

// global keyword tests

#[test]
fn test_global_read() {
    assert_eq!(run_php(r#"<?php
$x = 10;
function foo() {
    global $x;
    echo $x;
}
foo();
"#), "10");
}

#[test]
fn test_global_write() {
    assert_eq!(run_php(r#"<?php
$x = 10;
function foo() {
    global $x;
    $x = 20;
}
foo();
echo $x;
"#), "20");
}

#[test]
fn test_global_multiple() {
    assert_eq!(run_php(r#"<?php
$a = 1;
$b = 2;
function swap() {
    global $a, $b;
    $tmp = $a;
    $a = $b;
    $b = $tmp;
}
swap();
echo "$a $b";
"#), "2 1");
}

// static variable tests

#[test]
fn test_static_counter() {
    assert_eq!(run_php(r#"<?php
function counter() {
    static $count = 0;
    $count++;
    return $count;
}
echo counter() . " " . counter() . " " . counter();
"#), "1 2 3");
}

#[test]
fn test_static_multiple_vars() {
    assert_eq!(run_php(r#"<?php
function test() {
    static $a = 0, $b = 10;
    $a++;
    $b--;
    echo "$a:$b ";
}
test();
test();
"#), "1:9 2:8 ");
}

#[test]
fn test_static_default_null() {
    assert_eq!(run_php(r#"<?php
function test() {
    static $x;
    if ($x === null) {
        $x = "initialized";
    }
    echo $x . " ";
    $x = "modified";
}
test();
test();
"#), "initialized modified ");
}
