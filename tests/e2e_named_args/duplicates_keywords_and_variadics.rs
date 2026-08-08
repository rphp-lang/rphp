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
