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
fn iterator_to_array_collects_generators_and_controls_key_preservation() {
    assert_eq!(
        run_php(
            r#"<?php
function keyedValues(): Generator {
    yield 4 => 'first';
    yield 'name' => 'middle';
    yield 4 => 'last';
}
var_dump(iterator_to_array(keyedValues()));
var_dump(iterator_to_array(keyedValues(), false));
"#,
        ),
        concat!(
            "array(2) {\n  [4]=>\n  string(4) \"last\"\n  [\"name\"]=>\n  string(6) \"middle\"\n}\n",
            "array(3) {\n  [0]=>\n  string(5) \"first\"\n  [1]=>\n  string(6) \"middle\"\n  [2]=>\n  string(4) \"last\"\n}\n",
        )
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

#[test]
fn test_typed_generator_completion_and_internal_return_value() {
    assert_eq!(
        run_php(
            r#"<?php
function typedValues(): Generator {
    yield 1;
    return 7;
}
class TypedGeneratorFactory {
    public function values(): Generator {
        yield 2;
    }
}
$closure = function (): Generator {
    yield 3;
};
$generator = typedValues();
foreach ($generator as $value) { echo $value; }
echo ":" . $generator->getReturn() . ":";
foreach ((new TypedGeneratorFactory())->values() as $value) { echo $value; }
echo ":";
foreach ($closure() as $value) { echo $value; }
"#
        ),
        "1:7:2:3"
    );
}

#[test]
fn test_generator_return_contract_uses_iterator_hierarchy() {
    assert_eq!(
        run_php(
            r#"<?php
function traversableValues(): Traversable { yield 1; }
function iteratorValues(): Iterator { yield 2; }
function iterableValues(): iterable { yield 3; }
function objectValues(): object { yield 4; }
function nullableValues(): ?Generator { yield 5; }
function consume(iterable $values): string {
    $result = "";
    foreach ($values as $value) { $result .= $value; }
    return $result;
}
$generator = traversableValues();
echo ($generator instanceof Iterator ? "i" : "x");
echo ($generator instanceof Traversable ? "t" : "x");
echo consume($generator);
echo consume(iteratorValues());
echo consume(iterableValues());
echo consume(objectValues());
echo consume(nullableValues());
echo consume([6]);
"#
        ),
        "it123456"
    );

    use common::run_php_expect_error;
    let error = run_php_expect_error(
        r#"<?php
function invalidGenerator(): int { yield 1; }
invalidGenerator();
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Generator return type must be a supertype of Generator")
            && rendered.contains("int"),
        "{rendered}"
    );
}

#[test]
fn test_generator_exception_closes_and_reaches_foreach_caller() {
    assert_eq!(
        run_php(
            r#"<?php
function failingValues() {
    yield 1;
    throw new Exception("boom");
}
$generator = failingValues();
try {
    foreach ($generator as $value) { echo $value; }
} catch (Throwable $error) {
    echo ":" . $error->getMessage() . ":";
}
echo $generator->valid() ? "open" : "closed";

function immediateFailure() {
    throw new Exception("early");
    yield 0;
}
$immediate = immediateFailure();
try {
    foreach ($immediate as $value) { echo "unreachable"; }
} catch (Throwable $error) {
    echo ":" . $error->getMessage() . ":";
}
echo $immediate->valid() ? "open" : "closed";
"#
        ),
        "1:boom:closed:early:closed"
    );
}

#[test]
fn test_generator_exception_reaches_method_caller_and_yield_from_catch() {
    assert_eq!(
        run_php(
            r#"<?php
function directFailure() {
    yield 1;
    throw new Exception("direct");
}
$direct = directFailure();
echo $direct->current() . ":";
try {
    $direct->next();
} catch (Throwable $error) {
    echo $error->getMessage() . ":";
}

function innerFailure() {
    yield 2;
    throw new Exception("delegated");
}
function outerRecovery() {
    try {
        yield from innerFailure();
    } catch (Throwable $error) {
        yield $error->getMessage();
    }
    yield 3;
}
foreach (outerRecovery() as $value) { echo $value . ":"; }
"#
        ),
        "1:direct:2:delegated:3:"
    );
}

#[test]
fn test_generator_resume_preserves_or_replaces_pending_finally_exception() {
    assert_eq!(
        run_php(
            r#"<?php
function oneValue() { yield 1; }
try {
    try {
        throw new Exception("original");
    } finally {
        foreach (oneValue() as $value) { echo $value . ":"; }
    }
} catch (Throwable $error) {
    echo $error->getMessage() . ":";
}

function replacementFailure() {
    throw new Exception("replacement");
    yield 0;
}
try {
    try {
        throw new Exception("suppressed");
    } finally {
        $replacement = replacementFailure();
        $replacement->current();
    }
} catch (Throwable $error) {
    echo $error->getMessage();
}
"#
        ),
        "1:original:replacement"
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

#[test]
fn test_generator_yield_inside_method_argument() {
    assert_eq!(
        run_php(
            r#"<?php
class YieldReceiver {
    public function emit($value) {
        echo $value, ":";
    }
}
function values() {
    $receiver = new YieldReceiver;
    $receiver->emit(yield "ready");
}
$generator = values();
echo $generator->current(), ":";
$generator->send("sent");
"#
        ),
        "ready:sent:",
    );
}

#[test]
fn test_nullsafe_call_skips_yielding_argument() {
    assert_eq!(
        run_php(
            r#"<?php
function values() {
    $receiver = null;
    $receiver?->missing(yield "unreachable");
    echo "completed";
}
$generator = values();
$generator->current();
"#
        ),
        "completed",
    );
}

#[test]
fn test_generator_yield_inside_other_call_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
function emitYielded($value) { echo "f" . $value . ":"; }
class StaticYieldReceiver {
    public static function emit($value) { echo "s" . $value . ":"; }
}
function globalCall() { emitYielded(yield "global"); }
function staticCall() { StaticYieldReceiver::emit(yield "static"); }
function dynamicCall() {
    $callable = function ($value) { echo "d" . $value . ":"; };
    $callable(yield "dynamic");
}
$generator = globalCall();
echo $generator->current(), ":";
$generator->send("1");
$generator = staticCall();
echo $generator->current(), ":";
$generator->send("2");
$generator = dynamicCall();
echo $generator->current(), ":";
$generator->send("3");
"#
        ),
        "global:f1:static:s2:dynamic:d3:",
    );
}

#[test]
fn test_generator_yield_call_preserves_prior_reference_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
function replace(&$target, $value) { $target = $value; }
class YieldReferenceReceiver {
    public function replace(&$target, $value) { $target = $value; }
}
function globalReference() {
    $value = "old";
    replace($value, yield "global");
    echo $value, ":";
}
function methodReference() {
    $value = "old";
    $receiver = new YieldReferenceReceiver;
    $receiver->replace($value, yield "method");
    echo $value, ":";
}
$generator = globalReference();
echo $generator->current(), ":";
$generator->send("new-global");
$generator = methodReference();
echo $generator->current(), ":";
$generator->send("new-method");
"#
        ),
        "global:new-global:method:new-method:",
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

#[test]
fn semi_reserved_from_is_valid_for_functions_and_static_members() {
    assert_eq!(
        run_php(
            r#"<?php
function from() { yield 1; yield 2; }
class Factory {
    public static function from() { return "method"; }
}
foreach (from() as $value) echo $value;
echo ":", Factory::from();
"#
        ),
        "12:method"
    );
}
