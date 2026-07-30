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
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::function::CallStrategy;

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

#[test]
fn test_direct_scalar_call_chain_gets_fast_scalar_plan() {
    let source = "<?php
function leaf($x) { return $x + 1; }
function chain($x) { return leaf($x); }
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let chain = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("chain"))
        .map(|(_, function)| function)
        .unwrap();

    assert!(!chain.op_array.may_access_globals);
    assert_eq!(chain.common.plan.call, CallStrategy::FastScalar);
}

#[test]
fn test_transitive_global_chain_stays_conservative() {
    let source = "<?php
function reader() { global $value; return $value; }
function chain() { return reader(); }
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let chain = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("chain"))
        .map(|(_, function)| function)
        .unwrap();

    assert!(chain.op_array.may_access_globals);
    assert_ne!(chain.common.plan.call, CallStrategy::FastScalar);
}

#[test]
fn test_leaf_scalar_method_gets_fast_scalar_plan() {
    let source = "<?php
class Math {
    public function add($a, $b) { return $a + $b; }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let method = &result.class_defs[0].methods[0].4;

    assert_eq!(method.common.sig.this_offset, 1);
    assert_eq!(method.common.sig.public_arity(), 2);
    assert_eq!(method.common.plan.call, CallStrategy::FastScalar);
    assert!(method.common.plan.borrow_this);
}

#[test]
fn test_method_with_dynamic_dispatch_stays_conservative() {
    let source = "<?php
class Wrapper {
    public function run($other, $value) { return $other->apply($value); }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let method = &result.class_defs[0].methods[0].4;

    assert!(method.op_array.may_access_globals);
    assert_ne!(method.common.plan.call, CallStrategy::FastScalar);
}

#[test]
fn test_method_returning_this_keeps_owned_receiver() {
    let source = "<?php
class Identity {
    public function me() { return $this; }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let method = &result.class_defs[0].methods[0].4;

    assert_eq!(method.common.plan.call, CallStrategy::FastScalar);
    assert!(!method.common.plan.borrow_this);
    assert_eq!(run_php("<?php
class Identity {
    public function me() { return $this; }
}
$object = new Identity();
echo $object->me() === $object ? 'same' : 'different';
"), "same");
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

// ══════════════════════════════════════════════════════════════════════
// 9. FetchObjR + AssignObjProp: property access in hot tier
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_property_read_write() {
    // Method reads and writes public properties via inline cache.
    // Property values are scalar → stays fully in hot tier.
    assert_eq!(run_php("<?php
class Counter {
    public $val = 0;
    public function inc() {
        $this->val = $this->val + 1;
    }
    public function get() { return $this->val; }
}
$c = new Counter();
for ($i = 0; $i < 100; $i++) { $c->inc(); }
echo $c->get();
"), "100");
}

#[test]
fn test_hot_property_multiple_fields() {
    // Multiple property reads/writes per method call.
    assert_eq!(run_php("<?php
class Stats {
    public $count = 0;
    public $sum = 0;
    public function record($v) {
        $this->count = $this->count + 1;
        $this->sum = $this->sum + $v;
    }
}
$st = new Stats();
for ($i = 1; $i <= 100; $i++) { $st->record($i); }
echo $st->count . '|' . $st->sum;
"), "100|5050");
}

#[test]
fn test_hot_property_conditional_update() {
    // Property update inside conditional — exercises FetchObjR in comparison.
    assert_eq!(run_php("<?php
class MinMax {
    public $min = 999;
    public $max = 0;
    public function update($v) {
        if ($v < $this->min) { $this->min = $v; }
        if ($v > $this->max) { $this->max = $v; }
    }
}
$mm = new MinMax();
for ($i = 50; $i >= 1; $i--) { $mm->update($i); }
for ($i = 51; $i <= 100; $i++) { $mm->update($i); }
echo $mm->min . '|' . $mm->max;
"), "1|100");
}

// ══════════════════════════════════════════════════════════════════════
// 10. InitMethodCall: method dispatch in hot tier
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_method_recursion_fib() {
    // fib via $this->fib() — recursive method call through InitMethodCall.
    assert_eq!(run_php("<?php
class MathHelper {
    public function fib($n) {
        if ($n <= 1) return $n;
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
$m = new MathHelper();
echo $m->fib(20);
"), "6765");
}

#[test]
fn test_hot_method_recursion_power() {
    // Recursive power via method call.
    assert_eq!(run_php("<?php
class Calc {
    public function pow($b, $e) {
        if ($e <= 0) return 1;
        return $b * $this->pow($b, $e - 1);
    }
}
$c = new Calc();
echo $c->pow(2, 20);
"), "1048576");
}

#[test]
fn test_hot_method_with_property_recursion() {
    // Combines InitMethodCall + FetchObjR in recursive context.
    // Method reads property, recurses, updates property.
    assert_eq!(run_php("<?php
class Accumulator {
    public $total = 0;
    public function add_range($from, $to) {
        if ($from > $to) return $this->total;
        $this->total = $this->total + $from;
        return $this->add_range($from + 1, $to);
    }
}
$a = new Accumulator();
echo $a->add_range(1, 100);
"), "5050");
}

// ══════════════════════════════════════════════════════════════════════
// 11. Method bailout: heap return / non-scalar boundaries
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_method_return_this_bailout() {
    // Fluent method returns $this (object/heap) → HeapReturnValue bail.
    // Baseline handles correctly after bail.
    assert_eq!(run_php("<?php
class Builder {
    public $val = 0;
    public function inc() { $this->val = $this->val + 1; return $this; }
    public function get() { return $this->val; }
}
$b = new Builder();
for ($i = 0; $i < 50; $i++) { $b->inc(); }
echo $b->get();
"), "50");
}

#[test]
fn test_hot_method_mixed_scalar_and_object_return() {
    // Method sometimes returns scalar, sometimes called in chain.
    // Tests transition between hot completion and bailout.
    assert_eq!(run_php("<?php
class Acc {
    public $v = 0;
    public function add($x) { $this->v = $this->v + $x; }
    public function result() { return $this->v; }
}
$a = new Acc();
for ($i = 0; $i < 100; $i++) { $a->add($i); }
echo $a->result();
"), "4950");
}

// ══════════════════════════════════════════════════════════════════════
// 12. Static methods: already work via InitFcall path
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_static_method_calls() {
    // Static methods go through InitStaticCall → DoFcall standard path.
    // Should work at 100% in hot tier.
    assert_eq!(run_php("<?php
class Math {
    public static function add($a, $b) { return $a + $b; }
    public static function mul($a, $b) { return $a * $b; }
}
$r = 0;
for ($i = 0; $i < 100; $i++) {
    $r = Math::add($r, Math::mul($i, 2));
}
echo $r;
"), "9900");
}

// ══════════════════════════════════════════════════════════════════════
// 13. Multi-class: method calls across different classes
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_method_different_classes() {
    // Two different classes with scalar-return methods called in loop.
    // InitMethodCall IC is monomorphic — different class_id causes re-resolve.
    assert_eq!(run_php("<?php
class Adder {
    public function apply($x) { return $x + 1; }
}
class Doubler {
    public function apply($x) { return $x * 2; }
}
$a = new Adder();
$d = new Doubler();
$r1 = 0; $r2 = 0;
for ($i = 0; $i < 50; $i++) { $r1 = $a->apply($r1); }
for ($i = 0; $i < 50; $i++) { $r2 = $d->apply($r2); }
echo $r1 . '|' . $r2;
"), "50|0");
}

#[test]
fn test_hot_method_property_across_instances() {
    // Same class, different instances — inline cache should hit for both
    // since class_id is identical.
    assert_eq!(run_php("<?php
class Box {
    public $val;
    public function __construct($v) { $this->val = $v; }
    public function add($x) { $this->val = $this->val + $x; }
    public function get() { return $this->val; }
}
$a = new Box(0);
$b = new Box(100);
for ($i = 0; $i < 50; $i++) {
    $a->add(1);
    $b->add(2);
}
echo $a->get() . '|' . $b->get();
"), "50|200");
}
