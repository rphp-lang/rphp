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
