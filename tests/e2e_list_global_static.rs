mod common;
use common::run_php;

// list() / destructuring tests

#[test]
fn test_list_basic() {
    assert_eq!(
        run_php(
            r#"<?php
list($a, $b, $c) = [10, 20, 30];
echo "$a $b $c";
"#
        ),
        "10 20 30"
    );
}

#[test]
fn test_short_destructuring() {
    assert_eq!(
        run_php(
            r#"<?php
[$a, $b] = [1, 2];
echo "$a $b";
"#
        ),
        "1 2"
    );
}

#[test]
fn test_list_skip_elements() {
    assert_eq!(
        run_php(
            r#"<?php
[, $b, , $d] = [1, 2, 3, 4];
echo "$b $d";
"#
        ),
        "2 4"
    );
}

#[test]
fn test_list_from_function() {
    assert_eq!(
        run_php(
            r#"<?php
function pair() { return [42, "hello"]; }
[$num, $str] = pair();
echo "$num $str";
"#
        ),
        "42 hello"
    );
}

// global keyword tests

#[test]
fn test_global_read() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 10;
function foo() {
    global $x;
    echo $x;
}
foo();
"#
        ),
        "10"
    );
}

#[test]
fn test_global_write() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 10;
function foo() {
    global $x;
    $x = 20;
}
foo();
echo $x;
"#
        ),
        "20"
    );
}

#[test]
fn test_global_multiple() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "2 1"
    );
}

// Transitive global access tests — A calls B, B uses `global`
// These verify that the `needs_globals_sync` guard correctly syncs
// caller scope even when the immediate callee doesn't use `global`.

#[test]
fn test_global_transitive_one_level() {
    // A() has no `global`, A() calls B(), B() has `global $x`
    assert_eq!(
        run_php(
            r#"<?php
$x = 42;
function A() {
    B();
}
function B() {
    global $x;
    echo $x;
}
A();
"#
        ),
        "42"
    );
}

#[test]
fn test_global_transitive_two_levels() {
    // A() calls B(), B() calls C(), C() has `global $x`
    assert_eq!(
        run_php(
            r#"<?php
$x = 99;
function A() {
    B();
}
function B() {
    C();
}
function C() {
    global $x;
    echo $x;
}
A();
"#
        ),
        "99"
    );
}

#[test]
fn test_global_transitive_write() {
    // A() calls B(), B() writes `global $x`, verify main scope sees the change
    assert_eq!(
        run_php(
            r#"<?php
$x = 1;
function A() {
    B();
}
function B() {
    global $x;
    $x = 200;
}
A();
echo $x;
"#
        ),
        "200"
    );
}

#[test]
fn test_global_transitive_closure() {
    // Closure calls a function that uses `global`
    assert_eq!(
        run_php(
            r#"<?php
$x = 77;
function reader() {
    global $x;
    return $x;
}
$f = function() {
    return reader();
};
echo $f();
"#
        ),
        "77"
    );
}

#[test]
fn test_global_transitive_method() {
    // Method calls a function that uses `global`
    assert_eq!(
        run_php(
            r#"<?php
$x = 55;
function get_global_x() {
    global $x;
    return $x;
}
class Foo {
    function bar() {
        return get_global_x();
    }
}
$obj = new Foo();
echo $obj->bar();
"#
        ),
        "55"
    );
}

#[test]
fn test_global_after_modification_transitive() {
    // Modify $x in main scope, then A() → B() reads it
    assert_eq!(
        run_php(
            r#"<?php
$x = 1;
function A() { B(); }
function B() { global $x; echo $x . " "; }
A();
$x = 2;
A();
$x = 3;
A();
"#
        ),
        "1 2 3 "
    );
}

// static variable tests

#[test]
fn test_static_counter() {
    assert_eq!(
        run_php(
            r#"<?php
function counter() {
    static $count = 0;
    $count++;
    return $count;
}
echo counter() . " " . counter() . " " . counter();
"#
        ),
        "1 2 3"
    );
}

#[test]
fn test_static_multiple_vars() {
    assert_eq!(
        run_php(
            r#"<?php
function test() {
    static $a = 0, $b = 10;
    $a++;
    $b--;
    echo "$a:$b ";
}
test();
test();
"#
        ),
        "1:9 2:8 "
    );
}

#[test]
fn test_static_default_null() {
    assert_eq!(
        run_php(
            r#"<?php
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
"#
        ),
        "initialized modified "
    );
}

#[test]
fn test_method_local_static_variable() {
    assert_eq!(
        run_php(
            r#"<?php
class Counter {
    public function next() {
        static $count = 0;
        return ++$count;
    }
}
$counter = new Counter();
echo $counter->next(), '|', $counter->next();
"#,
        ),
        "1|2"
    );
}

#[test]
fn test_destructuring_assignment_is_value_producing_and_allows_skips() {
    assert_eq!(
        run_php(
            "<?php if ([$first, , $third] = ['a', 'ignored', 'c']) { echo $first . $third . ':' . implode(',', [$first, $third]); }"
        ),
        "ac:a,c"
    );
}
