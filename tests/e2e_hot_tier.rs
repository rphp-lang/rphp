/// E2E tests: hot tier — promotion, bailout, and tier transition correctness.
///
/// These tests exercise the tiering mechanics in `hot.rs`:
/// - Promotion: functions crossing call threshold become Hot
/// - Bailout: hot executor returns to baseline on unsupported patterns
/// - Correctness: hot path produces identical results to baseline
/// - Eligibility: ineligible functions (typed params, globals, generators) stay Cold
///
/// Test strategy: functions are called >FUNC_HOT_THRESHOLD times to ensure
/// both cold and hot paths are exercised. Correctness is verified by output.

mod common;
use common::run_php;

// ══════════════════════════════════════════════════════════════════════
// 1. Promotion: scalar recursion enters hot executor
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_scalar_recursion_fib() {
    // fib(25) generates ~150K recursive calls → well past threshold.
    // Hot executor handles the entire recursive tree after promotion.
    assert_eq!(run_php("<?php
function fib($n) {
    if ($n <= 1) return $n;
    return fib($n - 1) + fib($n - 2);
}
echo fib(25);
"), "75025");
}

#[test]
fn test_hot_scalar_recursion_factorial() {
    // Tail-ish recursion with multiplication.
    assert_eq!(run_php("<?php
function fact($n) {
    if ($n <= 1) return 1;
    return $n * fact($n - 1);
}
echo fact(10);
"), "3628800");
}

#[test]
fn test_hot_many_iterations_same_function() {
    // Non-recursive but called many times → crosses threshold via loop.
    assert_eq!(run_php("<?php
function add($a, $b) { return $a + $b; }
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum = add($sum, $i);
}
echo $sum;
"), "4950");
}

// ══════════════════════════════════════════════════════════════════════
// 2. Bailout: hot executor correctly falls back to baseline
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_bailout_unsupported_opcode_echo() {
    // Echo is not handled by hot executor → bails on UnsupportedOpcode.
    // Baseline picks up and executes correctly.
    assert_eq!(run_php("<?php
function greet($n) {
    if ($n <= 0) {
        echo 'done';
        return 0;
    }
    return greet($n - 1) + 1;
}
echo greet(20) . '|';
"), "done20|");
}

#[test]
fn test_hot_bailout_string_concat_in_hot_func() {
    // String concatenation produces heap values → hot executor bails.
    // Function starts scalar-recursive (promoted), then returns heap
    // from base case — Return bails on heap value.
    assert_eq!(run_php("<?php
function build($n) {
    if ($n <= 0) return 'x';
    return build($n - 1) . 'y';
}
echo build(15);
"), "xyyyyyyyyyyyyyyy");
}

#[test]
fn test_hot_bailout_heap_sendval() {
    // hot_func calls another function with a heap (string) argument.
    // SendVal for heap value bails to baseline.
    assert_eq!(run_php("<?php
function identity($x) { return $x; }
function caller($n) {
    if ($n <= 0) return identity('hello');
    return caller($n - 1);
}
echo caller(20);
"), "hello");
}

// ══════════════════════════════════════════════════════════════════════
// 3. Eligibility: ineligible functions stay Cold
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_typed_params_stay_cold() {
    // int type hint → can_promote_to_hot() returns false.
    // Function runs correctly in baseline even after many calls.
    assert_eq!(run_php("<?php
function typed_add(int $a, int $b): int {
    return $a + $b;
}
$sum = 0;
for ($i = 0; $i < 50; $i++) {
    $sum = typed_add($sum, $i);
}
echo $sum;
"), "1225");
}

#[test]
fn test_global_function_stays_cold() {
    // global keyword → ReturnStrategy::Full → can_promote_to_hot() false.
    assert_eq!(run_php("<?php
$counter = 0;
function inc() {
    global $counter;
    $counter++;
    return $counter;
}
for ($i = 0; $i < 20; $i++) { inc(); }
echo $counter;
"), "20");
}

#[test]
fn test_generator_stays_cold() {
    // Generator functions should never be promoted (generator implies Full return).
    assert_eq!(run_php("<?php
function gen_range($start, $end) {
    for ($i = $start; $i <= $end; $i++) {
        yield $i;
    }
}
$sum = 0;
foreach (gen_range(1, 20) as $v) {
    $sum += $v;
}
echo $sum;
"), "210");
}

#[test]
fn test_try_finally_stays_cold() {
    // try/finally → ReturnStrategy::Full → stays Cold.
    assert_eq!(run_php("<?php
function safe_div($a, $b) {
    try {
        if ($b == 0) throw new Exception('div by zero');
        return $a / $b;
    } finally {
        // cleanup — ensures Full return
    }
}
$sum = 0;
for ($i = 1; $i <= 20; $i++) {
    $sum += safe_div(100, $i);
}
echo intval($sum);
"), "359");
}

// ══════════════════════════════════════════════════════════════════════
// 4. Tier transitions: hot ↔ baseline interleave
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_calls_cold_callee() {
    // hot_func is scalar-recursive (promoted), calls cold_func which uses echo.
    // hot executor bails on cold_func → baseline handles it → hot resumes caller.
    assert_eq!(run_php("<?php
function cold_func($n) {
    echo $n;
    return $n;
}
function hot_func($n) {
    if ($n <= 0) return cold_func(0);
    return hot_func($n - 1) + 1;
}
echo '|' . hot_func(15) . '|';
"), "0|15|");
}

#[test]
fn test_hot_recursive_with_cold_leaf() {
    // Deep hot recursion, base case calls strlen() (internal function).
    // Internal functions in DoFcall → IneligibleCallee bail → baseline handles.
    assert_eq!(run_php("<?php
function deep($n) {
    if ($n <= 0) return strlen('hello');
    return deep($n - 1) + 1;
}
echo deep(20);
"), "25");
}

#[test]
fn test_two_hot_functions_mutual() {
    // Two mutually recursive functions, both become hot.
    // Tests that hot executor handles calls to OTHER hot functions correctly.
    assert_eq!(run_php("<?php
function is_even($n) {
    if ($n == 0) return 1;
    return is_odd($n - 1);
}
function is_odd($n) {
    if ($n == 0) return 0;
    return is_even($n - 1);
}
echo is_even(20) . is_odd(20) . is_even(21) . is_odd(21);
"), "1001");
}

// ══════════════════════════════════════════════════════════════════════
// 5. Threshold boundary: exact promotion point
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_threshold_boundary_correctness() {
    // Call function exactly at and around the threshold.
    // Verifies no off-by-one in promotion logic.
    // FUNC_HOT_THRESHOLD = 8, so call 9+ times to see hot path.
    assert_eq!(run_php("<?php
function inc($n) { return $n + 1; }
$x = 0;
// Calls 1-7: Cold path
for ($i = 0; $i < 7; $i++) { $x = inc($x); }
echo $x . '|';
// Call 8: triggers promotion (cc == threshold)
$x = inc($x);
echo $x . '|';
// Calls 9+: Hot path
for ($i = 0; $i < 10; $i++) { $x = inc($x); }
echo $x;
"), "7|8|18");
}

// ══════════════════════════════════════════════════════════════════════
// 6. AssignCv edge cases in hot executor
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_assigncv_scalar_overwrite() {
    // Multiple scalar assignments within hot function.
    // Verifies raw_copy doesn't leak or corrupt.
    assert_eq!(run_php("<?php
function reassign($n) {
    if ($n <= 0) return 0;
    $x = $n;
    $x = $x - 1;
    $x = $x - 1;
    return reassign($x) + 2;
}
echo reassign(20);
"), "20");
}

#[test]
fn test_hot_assigncv_heap_destination_bail() {
    // Function receives string param (heap) from baseline, then assigns scalar to it.
    // Hot executor should bail on heap destination, baseline handles correctly.
    assert_eq!(run_php("<?php
function process($label, $n) {
    if ($n <= 0) return 0;
    $label = $n;  // overwrite heap CV with scalar — should bail
    return process('tag', $n - 1) + $label;
}
echo process('start', 15);
"), "120");
}

// ══════════════════════════════════════════════════════════════════════
// 7. Return value correctness across tiers
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_return_to_cold_caller() {
    // Main scope (cold) calls hot function and uses return value in expression.
    assert_eq!(run_php("<?php
function fib($n) {
    if ($n <= 1) return $n;
    return fib($n - 1) + fib($n - 2);
}
$a = fib(10);
$b = fib(15);
echo ($a + $b);
"), "665");
}

#[test]
fn test_hot_unused_return_value() {
    // Call hot function but discard return value (OpType::Unused result).
    assert_eq!(run_php("<?php
function counter($n) {
    if ($n <= 0) return 0;
    counter($n - 1);  // return value discarded
    return $n;
}
echo counter(15);
"), "15");
}

// ══════════════════════════════════════════════════════════════════════
// 8. Regression: correctness under hot executor for various patterns
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_ackermann() {
    // Ackermann function: deeply recursive, multiple branches.
    // ack(3,4) = 125, ~10K+ calls → exercises hot path thoroughly.
    assert_eq!(run_php("<?php
function ack($m, $n) {
    if ($m == 0) return $n + 1;
    if ($n == 0) return ack($m - 1, 1);
    return ack($m - 1, ack($m, $n - 1));
}
echo ack(3, 4);
"), "125");
}

#[test]
fn test_hot_gcd_euclidean() {
    // GCD via Euclidean algorithm — called many times in loop.
    assert_eq!(run_php("<?php
function gcd($a, $b) {
    if ($b == 0) return $a;
    return gcd($b, $a - intval($a / $b) * $b);
}
$sum = 0;
for ($i = 1; $i <= 30; $i++) {
    $sum += gcd(120, $i);
}
echo $sum;
"), "185");
}

#[test]
fn test_hot_power_recursive() {
    // Recursive power: exercises multiply in hot path.
    assert_eq!(run_php("<?php
function power($base, $exp) {
    if ($exp <= 0) return 1;
    return $base * power($base, $exp - 1);
}
echo power(2, 20) . '|' . power(3, 10);
"), "1048576|59049");
}
