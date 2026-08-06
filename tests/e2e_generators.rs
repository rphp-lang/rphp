/// Tests for PHP generators (yield)
mod common;
use common::run_php;

// ── Basic generators ──

#[test]
fn test_generator_basic_yield() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield 1;
    yield 2;
    yield 3;
}
$g = gen();
echo $g->current();
$g->next();
echo $g->current();
$g->next();
echo $g->current();
"#
        ),
        "123"
    );
}

#[test]
fn test_generator_foreach() {
    assert_eq!(
        run_php(
            r#"<?php
function nums() {
    yield 10;
    yield 20;
    yield 30;
}
foreach (nums() as $n) {
    echo $n . " ";
}
"#
        ),
        "10 20 30 "
    );
}

#[test]
fn test_generator_valid() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield 1;
}
$g = gen();
echo $g->valid() ? "yes" : "no";
$g->next();
echo $g->valid() ? "yes" : "no";
"#
        ),
        "yesno"
    );
}

#[test]
fn test_generator_key_auto() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield "a";
    yield "b";
    yield "c";
}
foreach (gen() as $k => $v) {
    echo $k . ":" . $v . " ";
}
"#
        ),
        "0:a 1:b 2:c "
    );
}

#[test]
fn test_generator_key_explicit() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield "x" => 10;
    yield "y" => 20;
}
foreach (gen() as $k => $v) {
    echo $k . "=" . $v . " ";
}
"#
        ),
        "x=10 y=20 "
    );
}

// ── Generator with parameters ──

#[test]
fn test_generator_with_params() {
    assert_eq!(
        run_php(
            r#"<?php
function range_gen($start, $end) {
    for ($i = $start; $i <= $end; $i++) {
        yield $i;
    }
}
foreach (range_gen(3, 7) as $n) {
    echo $n . " ";
}
"#
        ),
        "3 4 5 6 7 "
    );
}

// ── Generator return value ──

#[test]
fn test_generator_return_value() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield 1;
    yield 2;
    return "done";
}
$g = gen();
$g->next();
$g->next();
echo $g->getReturn();
"#
        ),
        "done"
    );
}

// ── send() ──

#[test]
fn test_generator_send() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    $x = yield 1;
    echo $x;
    $y = yield 2;
    echo $y;
}
$g = gen();
echo $g->current();
$g->send("A");
echo $g->current();
$g->send("B");
"#
        ),
        "1A2B"
    );
}

// ── Generator with local state ──

#[test]
fn test_generator_fibonacci() {
    assert_eq!(
        run_php(
            r#"<?php
function fib() {
    $a = 0;
    $b = 1;
    while (true) {
        yield $a;
        $temp = $a + $b;
        $a = $b;
        $b = $temp;
    }
}
$g = fib();
$result = "";
for ($i = 0; $i < 8; $i++) {
    $result = $result . $g->current() . " ";
    $g->next();
}
echo $result;
"#
        ),
        "0 1 1 2 3 5 8 13 "
    );
}

// ── Empty generator ──

#[test]
fn test_generator_empty() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    if (false) {
        yield 1;
    }
}
$g = gen();
echo $g->valid() ? "yes" : "no";
"#
        ),
        "no"
    );
}

// ── Multiple generators independent ──

#[test]
fn test_generator_multiple_independent() {
    assert_eq!(
        run_php(
            r#"<?php
function counter($start) {
    $i = $start;
    while (true) {
        yield $i;
        $i++;
    }
}
$a = counter(1);
$b = counter(10);
echo $a->current();
echo $b->current();
$a->next();
echo $a->current();
echo $b->current();
"#
        ),
        "110210"
    );
}

// ── yield null ──

#[test]
fn test_generator_yield_null() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield;
    yield;
}
$g = gen();
echo $g->valid() ? "valid" : "invalid";
echo $g->current() === null ? " null" : " other";
"#
        ),
        "valid null"
    );
}

// ── Foreach with key ──

#[test]
fn test_generator_foreach_multiple_yields_in_loop() {
    assert_eq!(
        run_php(
            r#"<?php
function pairs() {
    $items = ["a", "b", "c"];
    foreach ($items as $i => $item) {
        yield $i => $item;
    }
}
$result = "";
foreach (pairs() as $k => $v) {
    $result = $result . $k . $v;
}
echo $result;
"#
        ),
        "0a1b2c"
    );
}

// ── Generator as method (basic, without $this for now) ──

#[test]
fn test_generator_yield_expression_value() {
    // yield as expression returns null when next() is used (no send value)
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    $val = yield 42;
    if ($val === null) {
        echo "null";
    } else {
        echo $val;
    }
}
$g = gen();
echo $g->current();
$g->next();
"#
        ),
        "42null"
    );
}

// ── yield from ──

#[test]
fn test_yield_from_generator() {
    assert_eq!(
        run_php(
            r#"<?php
function inner() {
    yield 1;
    yield 2;
    yield 3;
}
function outer() {
    yield from inner();
}
foreach (outer() as $v) {
    echo $v . " ";
}
"#
        ),
        "1 2 3 "
    );
}

#[test]
fn test_yield_from_array() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield from [10, 20, 30];
}
foreach (gen() as $v) {
    echo $v . " ";
}
"#
        ),
        "10 20 30 "
    );
}

#[test]
fn test_yield_from_return_value() {
    assert_eq!(
        run_php(
            r#"<?php
function inner() {
    yield 1;
    yield 2;
    return "done";
}
function outer() {
    $result = yield from inner();
    echo $result;
}
$g = outer();
$g->next();
$g->next();
$g->next();
"#
        ),
        "done"
    );
}

#[test]
fn test_yield_from_with_own_yields() {
    assert_eq!(
        run_php(
            r#"<?php
function inner() {
    yield 2;
    yield 3;
}
function outer() {
    yield 1;
    yield from inner();
    yield 4;
}
$result = "";
foreach (outer() as $v) {
    $result = $result . $v;
}
echo $result;
"#
        ),
        "1234"
    );
}

#[test]
fn test_yield_from_multiple() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield from [1, 2];
    yield from [3, 4];
}
$result = "";
foreach (gen() as $v) {
    $result = $result . $v;
}
echo $result;
"#
        ),
        "1234"
    );
}

#[test]
fn test_yield_from_send() {
    assert_eq!(
        run_php(
            r#"<?php
function inner() {
    $x = yield 1;
    echo $x;
    $y = yield 2;
    echo $y;
}
function outer() {
    yield from inner();
}
$g = outer();
echo $g->current();
$g->send("A");
echo $g->current();
$g->send("B");
"#
        ),
        "1A2B"
    );
}

#[test]
fn test_yield_from_empty_array() {
    assert_eq!(
        run_php(
            r#"<?php
function gen() {
    yield from [];
    yield 42;
}
foreach (gen() as $v) {
    echo $v . " ";
}
"#
        ),
        "42 "
    );
}

#[test]
fn test_yield_from_nested() {
    assert_eq!(
        run_php(
            r#"<?php
function a() {
    yield 1;
    yield 2;
}
function b() {
    yield from a();
    yield 3;
}
function c() {
    yield from b();
    yield 4;
}
$result = "";
foreach (c() as $v) {
    $result = $result . $v;
}
echo $result;
"#
        ),
        "1234"
    );
}

// ── send() on fresh generator (P1 fix) ──

#[test]
fn test_generator_send_on_fresh() {
    // PHP: send() on Created generator starts it, then injects send value
    assert_eq!(
        run_php(
            r#"<?php
function g() {
    $x = yield 1;
    yield $x;
}
$g = g();
echo $g->send("foo");
"#
        ),
        "foo"
    );
}

// ── new Generator() guard (P2 fix) ──

#[test]
fn test_new_generator_forbidden() {
    use common::run_php_expect_error;
    let err = run_php_expect_error(
        r#"<?php
$g = new Generator();
"#,
    );
    let msg = format!("{:?}", err);
    assert!(msg.contains("reserved for internal use"));
}

// ── yield from error is catchable (P1 fix) ──

#[test]
fn test_yield_from_invalid_catchable() {
    assert_eq!(
        run_php(
            r#"<?php
function g() {
    yield from 42;
}
try {
    $gen = g();
    $gen->current();
} catch (\Throwable $e) {
    echo "caught";
}
"#
        ),
        "caught"
    );
}
