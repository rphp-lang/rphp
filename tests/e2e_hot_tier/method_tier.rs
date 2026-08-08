// ══════════════════════════════════════════════════════════════════════
// 10. InitMethodCall: method dispatch in hot tier
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_method_recursion_fib() {
    // fib via $this->fib() — recursive method call through InitMethodCall.
    assert_eq!(
        run_php(
            "<?php
class MathHelper {
    public function fib($n) {
        if ($n <= 1) return $n;
        return $this->fib($n - 1) + $this->fib($n - 2);
    }
}
$m = new MathHelper();
echo $m->fib(20);
"
        ),
        "6765"
    );
}

#[test]
fn test_hot_method_recursion_power() {
    // Recursive power via method call.
    assert_eq!(
        run_php(
            "<?php
class Calc {
    public function pow($b, $e) {
        if ($e <= 0) return 1;
        return $b * $this->pow($b, $e - 1);
    }
}
$c = new Calc();
echo $c->pow(2, 20);
"
        ),
        "1048576"
    );
}

#[test]
fn test_hot_method_with_property_recursion() {
    // Combines InitMethodCall + FetchObjR in recursive context.
    // Method reads property, recurses, updates property.
    assert_eq!(
        run_php(
            "<?php
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
"
        ),
        "5050"
    );
}

#[test]
fn test_hot_recursive_caller_uses_direct_property_getter() {
    assert_eq!(
        run_php(
            "<?php
class Box {
    public $value = 7;
    public function value() { return $this->value; }
    public function repeated($n) {
        if ($n <= 0) return 0;
        return $this->value() + $this->repeated($n - 1);
    }
}
$box = new Box();
echo $box->repeated(20);
"
        ),
        "140"
    );
}

#[test]
fn test_hot_recursive_caller_uses_direct_property_mutator() {
    assert_eq!(
        run_php(
            "<?php
class Counter {
    public $value = 0;
    public function add($amount) {
        $this->value = $this->value + $amount;
    }
    public function repeated($n) {
        if ($n <= 0) return $this->value;
        $this->add(2);
        return $this->repeated($n - 1);
    }
}
$counter = new Counter();
$counter->repeated(1);
echo $counter->repeated(20);
"
        ),
        "42"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 11. Method bailout: heap return / non-scalar boundaries
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_method_return_this_bailout() {
    // Fluent method returns $this (object/heap) → HeapReturnValue bail.
    // Baseline handles correctly after bail.
    assert_eq!(
        run_php(
            "<?php
class Builder {
    public $val = 0;
    public function inc() { $this->val = $this->val + 1; return $this; }
    public function get() { return $this->val; }
}
$b = new Builder();
for ($i = 0; $i < 50; $i++) { $b->inc(); }
echo $b->get();
"
        ),
        "50"
    );
}

#[test]
fn test_hot_method_mixed_scalar_and_object_return() {
    // Method sometimes returns scalar, sometimes called in chain.
    // Tests transition between hot completion and bailout.
    assert_eq!(
        run_php(
            "<?php
class Acc {
    public $v = 0;
    public function add($x) { $this->v = $this->v + $x; }
    public function result() { return $this->v; }
}
$a = new Acc();
for ($i = 0; $i < 100; $i++) { $a->add($i); }
echo $a->result();
"
        ),
        "4950"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 12. Static methods: already work via InitFcall path
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_static_method_calls() {
    // Static methods go through InitStaticCall → DoFcall standard path.
    // Should work at 100% in hot tier.
    assert_eq!(
        run_php(
            "<?php
class Math {
    public static function add($a, $b) { return $a + $b; }
    public static function mul($a, $b) { return $a * $b; }
}
$r = 0;
for ($i = 0; $i < 100; $i++) {
    $r = Math::add($r, Math::mul($i, 2));
}
echo $r;
"
        ),
        "9900"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 13. Multi-class: method calls across different classes
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_method_different_classes() {
    // Two different classes with scalar-return methods called in loop.
    // InitMethodCall IC is monomorphic — different class_id causes re-resolve.
    assert_eq!(
        run_php(
            "<?php
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
"
        ),
        "50|0"
    );
}

#[test]
fn test_hot_method_property_across_instances() {
    // Same class, different instances — inline cache should hit for both
    // since class_id is identical.
    assert_eq!(
        run_php(
            "<?php
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
"
        ),
        "50|200"
    );
}

#[test]
fn test_hot_mixed_scalar_methods_feed_dynamic_hash_updates() {
    assert_eq!(
        run_php(
            "<?php
class MixedRouter {
    public function score(int $base, string $route): int {
        $value = $base + strlen($route);
        if ($route == 'left') {
            $value = $value + 7;
        } else if ($route == 'right') {
            $value = $value + 11;
        } else {
            $value = $value + 13;
        }
        return $value;
    }

    public function accepts(int $score, int $sequence): int {
        if (($score % 11) == 0 || ($sequence % 17) == 0) { return 1; }
        return 0;
    }
}

$router = new MixedRouter();
$totals = ['left' => 0, 'right' => 0, 'other' => 0];
$accepted = 0;
for ($i = 0; $i < 200; $i++) {
    $remainder = $i % 3;
    if ($remainder == 0) {
        $route = 'left';
    } else if ($remainder == 1) {
        $route = 'right';
    } else {
        $route = 'other';
    }
    $score = $router->score($i * 5, $route);
    $totals[$route] = $totals[$route] + $score;
    $accepted = $accepted + $router->accepts($score, $i);
}
echo $totals['left'] . '|' . $totals['right'] . '|';
echo $totals['other'] . '|' . $accepted;
"
        ),
        "33902|34572|34023|30"
    );
}

#[test]
fn test_object_long_plan_keeps_semantic_control_flow() {
    let source = "<?php
class LoweredPolicy {
    public function score(int $latency, int $bytes, string $route): int {
        $score = intdiv(($latency * 17) + $bytes, 13) + strlen($route);
        if ($route == 'write') {
            $score = $score + 37;
        } elseif ($route == 'delete') {
            $score = $score + 83;
        }
        if ($latency >= 300) { $score = $score + 101; }
        return $score;
    }
    public function accepts(int $score, int $sequence): int {
        if (($score % 11) == 0 || ($sequence % 17) == 0) { return 0; }
        return 1;
    }
}
$policy = new LoweredPolicy();
$policy->score(10, 60, 'write');
$policy->accepts(11, 1);
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let class = &result.class_defs[0];
    let score = class
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name.eq_ignore_ascii_case("score"))
        .map(|(_, _, _, _, method)| method)
        .unwrap();
    let accepted = class
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name.eq_ignore_ascii_case("accepts"))
        .map(|(_, _, _, _, method)| method)
        .unwrap();

    let score = score.object_long_plan.as_deref().unwrap();
    assert!(
        score
            .operations
            .iter()
            .any(|operation| matches!(operation, ObjectLongOp::Arithmetic { .. }))
    );
    assert!(
        score
            .operations
            .iter()
            .any(|operation| matches!(operation, ObjectLongOp::StringLength { .. }))
    );
    let weighted = score.weighted_string_score.as_deref().unwrap();
    assert_eq!(weighted.multiplier, 17);
    assert_eq!(weighted.divisor, 13);
    assert_eq!(weighted.string_adjustments.len(), 2);
    assert_eq!(weighted.conditional_adjustments.len(), 1);

    let accepted = accepted.object_long_plan.as_deref().unwrap();
    assert!(
        accepted
            .operations
            .iter()
            .any(|operation| matches!(operation, ObjectLongOp::Compare { .. }))
    );
    assert!(accepted.operations.iter().any(|operation| matches!(
        operation,
        ObjectLongOp::JumpIfFalse { .. } | ObjectLongOp::JumpIfTrue { .. }
    )));
    let modulo = accepted.modulo_any_select.as_deref().unwrap();
    assert_eq!(modulo.terms.len(), 2);
    assert_eq!(modulo.when_match, 0);
    assert_eq!(modulo.when_miss, 1);
}

#[test]
fn test_hot_weighted_string_score_preserves_all_adjustment_paths() {
    assert_eq!(
        run_php(
            "<?php
class WeightedScoreModel {
    public function score(int $latency, int $bytes, string $route): int {
        $score = intdiv(($latency * 17) + $bytes, 13) + strlen($route);
        if ($route == 'write') {
            $score = $score + 37;
        } elseif ($route == 'delete') {
            $score = $score + 83;
        }
        if ($latency >= 300) {
            $score = $score + 101;
        }
        return $score;
    }
}
$model = new WeightedScoreModel();
$warm = 0;
for ($i = 0; $i < 100; $i++) {
    $warm = $warm + $model->score(20, 128, 'read');
}
echo $warm . '|';
echo $model->score(20, 128, 'read') . '|';
echo $model->score(300, 128, 'write') . '|';
echo $model->score(419, 8319, 'delete');
"
        ),
        "4000|40|545|1377"
    );
}
