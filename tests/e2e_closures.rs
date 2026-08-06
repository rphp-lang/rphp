/// Tests for closures / anonymous functions
mod common;
use common::run_php;

#[test]
fn test_closure_basic() {
    assert_eq!(
        run_php(
            r#"<?php
$greet = function($name) {
    echo "Hello " . $name;
};
$greet("World");
"#
        ),
        "Hello World"
    );
}

#[test]
fn test_closure_with_use() {
    assert_eq!(
        run_php(
            r#"<?php
$prefix = "Hi";
$greet = function($name) use ($prefix) {
    echo $prefix . " " . $name;
};
$greet("PHP");
"#
        ),
        "Hi PHP"
    );
}

#[test]
fn test_closure_multiple_params() {
    assert_eq!(
        run_php(
            r#"<?php
$add = function($a, $b) {
    return $a + $b;
};
echo $add(3, 4);
"#
        ),
        "7"
    );
}

#[test]
fn test_closure_return_value() {
    assert_eq!(
        run_php(
            r#"<?php
$double = function($x) {
    return $x * 2;
};
$result = $double(21);
echo $result;
"#
        ),
        "42"
    );
}

#[test]
fn test_closure_use_captures_value() {
    // use captures by value — changing outer var doesn't affect closure
    assert_eq!(
        run_php(
            r#"<?php
$x = 10;
$fn = function() use ($x) {
    echo $x;
};
$x = 20;
$fn();
"#
        ),
        "10"
    );
}

#[test]
fn test_closure_multiple_use_vars() {
    assert_eq!(
        run_php(
            r#"<?php
$a = "foo";
$b = "bar";
$fn = function() use ($a, $b) {
    echo $a . " " . $b;
};
$fn();
"#
        ),
        "foo bar"
    );
}

#[test]
fn test_closure_no_params() {
    assert_eq!(
        run_php(
            r#"<?php
$fn = function() {
    echo "no params";
};
$fn();
"#
        ),
        "no params"
    );
}

#[test]
fn test_closure_as_callback() {
    // Pass closure to a function
    assert_eq!(
        run_php(
            r#"<?php
function apply($fn, $val) {
    return $fn($val);
}
$double = function($x) { return $x * 2; };
echo apply($double, 5);
"#
        ),
        "10"
    );
}

#[test]
fn test_multiple_closures() {
    assert_eq!(
        run_php(
            r#"<?php
$add = function($a, $b) { return $a + $b; };
$mul = function($a, $b) { return $a * $b; };
echo $add(2, 3) . " " . $mul(4, 5);
"#
        ),
        "5 20"
    );
}

#[test]
fn test_closure_use_with_params() {
    assert_eq!(
        run_php(
            r#"<?php
$base = 100;
$add_to_base = function($x) use ($base) {
    return $base + $x;
};
echo $add_to_base(42);
"#
        ),
        "142"
    );
}

#[test]
fn test_variable_function_call_string() {
    assert_eq!(
        run_php(
            r#"<?php
function greet($name) {
    echo "Hello " . $name;
}
$fn = "greet";
$fn("World");
"#
        ),
        "Hello World"
    );
}

#[test]
fn test_closure_body_compile_error_no_panic() {
    // break inside closure body should be a compile error, not a panic
    let tokens = rphp::lexer::Lexer::new("<?php $f = function() { break; };")
        .tokenize()
        .unwrap();
    let stmts = rphp::parser::Parser::new(tokens).parse().unwrap();
    let result = rphp::compiler::compile::Compiler::new().compile(&stmts);
    assert!(
        result.is_err(),
        "Expected compile error for break inside closure"
    );
}

// ============================================================================
// Arrow functions: fn($x) => expr
// ============================================================================

#[test]
fn test_arrow_basic() {
    assert_eq!(
        run_php(
            r#"<?php
$double = fn($x) => $x * 2;
echo $double(5);
"#
        ),
        "10"
    );
}

#[test]
fn test_arrow_multiple_params() {
    assert_eq!(
        run_php(
            r#"<?php
$add = fn($a, $b) => $a + $b;
echo $add(3, 7);
"#
        ),
        "10"
    );
}

#[test]
fn test_arrow_captures_outer_variable() {
    assert_eq!(
        run_php(
            r#"<?php
$factor = 3;
$mul = fn($x) => $x * $factor;
echo $mul(5);
"#
        ),
        "15"
    );
}

#[test]
fn test_arrow_captures_multiple_vars() {
    assert_eq!(
        run_php(
            r#"<?php
$prefix = "Hello";
$suffix = "!";
$greet = fn($name) => $prefix . " " . $name . $suffix;
echo $greet("PHP");
"#
        ),
        "Hello PHP!"
    );
}

#[test]
fn test_arrow_no_params() {
    assert_eq!(
        run_php(
            r#"<?php
$val = 42;
$get = fn() => $val;
echo $get();
"#
        ),
        "42"
    );
}

#[test]
fn test_arrow_with_default_param() {
    assert_eq!(
        run_php(
            r#"<?php
$f = fn($x, $y = 10) => $x + $y;
echo $f(5);
echo " ";
echo $f(5, 20);
"#
        ),
        "15 25"
    );
}

#[test]
fn test_arrow_nested() {
    assert_eq!(
        run_php(
            r#"<?php
$add = fn($a) => fn($b) => $a + $b;
$add5 = $add(5);
echo $add5(3);
"#
        ),
        "8"
    );
}

#[test]
fn test_arrow_in_array_map_style() {
    // Using arrow function as callback passed to another function
    assert_eq!(
        run_php(
            r#"<?php
function apply($fn, $val) {
    return $fn($val);
}
$square = fn($x) => $x * $x;
echo apply($square, 4);
"#
        ),
        "16"
    );
}

#[test]
fn test_arrow_with_string_concat() {
    assert_eq!(
        run_php(
            r#"<?php
$wrap = fn($s) => "[" . $s . "]";
echo $wrap("test");
"#
        ),
        "[test]"
    );
}

#[test]
fn test_arrow_with_ternary() {
    assert_eq!(
        run_php(
            r#"<?php
$abs = fn($x) => $x >= 0 ? $x : -$x;
echo $abs(5);
echo " ";
echo $abs(-3);
"#
        ),
        "5 3"
    );
}
