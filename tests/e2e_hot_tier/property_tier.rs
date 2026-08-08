// 9. FetchObjR + AssignObjProp: property access in hot tier
// ══════════════════════════════════════════════════════════════════════

#[test]
fn test_hot_property_read_write() {
    // Method reads and writes public properties via inline cache.
    // Property values are scalar → stays fully in hot tier.
    assert_eq!(
        run_php(
            "<?php
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
"
        ),
        "100"
    );
}

#[test]
fn test_hot_property_multiple_fields() {
    // Multiple property reads/writes per method call.
    assert_eq!(
        run_php(
            "<?php
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
"
        ),
        "100|5050"
    );
}

#[test]
fn test_hot_property_conditional_update() {
    // Property update inside conditional — exercises FetchObjR in comparison.
    assert_eq!(
        run_php(
            "<?php
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
"
        ),
        "1|100"
    );
}

#[test]
fn native_property_read_region_does_not_hoist_across_mutating_method() {
    assert_eq!(
        run_php(
            "<?php
class RunningCounter {
    public $value = 0;
    public function advance() { $this->value = $this->value + 1; }
}
$counter = new RunningCounter();
$sum = 0;
for ($i = 0; $i < 200; $i++) {
    $counter->advance();
    $sum += $counter->value;
}
echo $counter->value . '|' . $sum;
"
        ),
        "200|20100"
    );
}

#[test]
fn test_long_property_method_plan_is_compiled_from_general_patterns() {
    let source = "<?php
class Stats {
    public $count = 0;
    public $sum = 0;
    public $min = 999;
    public $max = 0;
    public function record($v) {
        $this->count = $this->count + 1;
        $this->sum = $this->sum + $v;
        if ($v < $this->min) { $this->min = $v; }
        if ($v > $this->max) { $this->max = $v; }
    }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let method = &result.class_defs[0].methods[0].4;
    let plan = method.long_property_plan.as_ref().unwrap();
    assert_eq!(plan.public_args, 1);
    assert_eq!(plan.properties.len(), 4);
    assert_eq!(plan.operations.len(), 4);
}

#[test]
fn test_property_getter_method_plan_is_compiled_for_exact_getter_only() {
    let source = "<?php
class Box {
    public $value = 7;
    public function value() { return $this->value; }
    public function adjusted() { return $this->value + 1; }
}
";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let getter = &result.class_defs[0].methods[0].4;
    let adjusted = &result.class_defs[0].methods[1].4;
    assert_eq!(getter.property_getter_plan.as_ref().unwrap().cache_ip, 0);
    assert!(adjusted.property_getter_plan.is_none());
}

#[test]
fn test_direct_property_getter_preserves_heap_value_cow() {
    assert_eq!(
        run_php(
            "<?php
class StringBox {
    public $value = 'base';
    public function value() { return $this->value; }
}
class ArrayBox {
    public $value = [1];
    public function value() { return $this->value; }
}
$strings = new StringBox();
$strings->value();
$stringCopy = $strings->value();
$stringCopy .= '!';
$arrays = new ArrayBox();
$arrays->value();
$arrayCopy = $arrays->value();
$arrayCopy[] = 2;
echo $strings->value() . '|' . count($arrays->value()) . '|' . count($arrayCopy);
"
        ),
        "base|1|2"
    );
}

#[test]
fn test_direct_property_getter_guards_polymorphism() {
    assert_eq!(
        run_php(
            "<?php
class First {
    public $value = 11;
    public function value() { return $this->value; }
}
class Second {
    public $value = 29;
    public function value() { return $this->value; }
}
function readValue($object) { return $object->value(); }
$first = new First();
$second = new Second();
echo readValue($first) . '|' . readValue($first) . '|';
echo readValue($second) . '|' . readValue($second) . '|';
echo readValue($first);
"
        ),
        "11|11|29|29|11"
    );
}

#[test]
fn test_property_getter_falls_back_for_private_and_magic_properties() {
    assert_eq!(
        run_php(
            "<?php
class PrivateBox {
    private $value = 17;
    public function value() { return $this->value; }
}
class MagicBox {
    public function __get($name) { return $name . '!'; }
    public function value() { return $this->missing; }
}
$private = new PrivateBox();
$magic = new MagicBox();
echo $private->value() . '|' . $private->value() . '|';
echo $magic->value() . '|' . $magic->value();
"
        ),
        "17|17|missing!|missing!"
    );
}

#[test]
fn test_long_property_method_plan_fallback_is_transactional_on_overflow() {
    // The first update in the second call must not be committed by the plan
    // before the overflowing second update falls back to ordinary execution.
    assert_eq!(
        run_php(
            "<?php
class Counter {
    public $large = 9223372036854775807;
    public $calls = 0;
    public function update($v) {
        $this->calls = $this->calls + 1;
        $this->large = $this->large + $v;
    }
}
$counter = new Counter();
$counter->update(0);
$counter->update(1);
echo $counter->calls;
"
        ),
        "2"
    );
}

#[test]
fn test_long_property_method_plan_does_not_replace_used_return_value() {
    assert_eq!(
        run_php(
            "<?php
class Counter {
    public $value = 0;
    public function add($v) {
        $this->value = $this->value + $v;
        return $this->value;
    }
}
$counter = new Counter();
$counter->add(1);
echo $counter->add(2) . '|' . $counter->value;
"
        ),
        "3|3"
    );
}

#[test]
fn test_deferred_property_method_evaluates_nested_argument_once() {
    assert_eq!(
        run_php(
            "<?php
class Accumulator {
    public $total = 0;
    public function add($value) {
        $this->total = $this->total + $value;
    }
}
$calls = 0;
function nextValue() {
    global $calls;
    $calls = $calls + 1;
    return 3;
}
$accumulator = new Accumulator();
$accumulator->add(0);
$accumulator->add(nextValue());
$accumulator->add(nextValue());
echo $accumulator->total . '|' . $calls;
"
        ),
        "6|2"
    );
}

#[test]
fn test_deferred_property_method_materializes_when_return_is_used() {
    assert_eq!(
        run_php(
            "<?php
class Accumulator {
    public $total = 0;
    public function add($value) {
        $this->total = $this->total + $value;
        return $this->total;
    }
}
$calls = 0;
function nextValue() {
    global $calls;
    $calls = $calls + 1;
    return 2;
}
$accumulator = new Accumulator();
$accumulator->add(0);
echo $accumulator->add(nextValue()) . '|' . $calls . '|' . $accumulator->total;
"
        ),
        "2|1|2"
    );
}

#[test]
fn test_deferred_property_method_overflow_fallback_is_transactional() {
    assert_eq!(
        run_php(
            "<?php
class Counter {
    public $large = 9223372036854775807;
    public $calls = 0;
    public function update($value) {
        $this->calls = $this->calls + 1;
        $this->large = $this->large + $value;
    }
}
function one() { return 1; }
$counter = new Counter();
$counter->update(0);
$counter->update(one());
echo $counter->calls;
"
        ),
        "2"
    );
}

#[test]
fn test_deferred_property_method_double_and_exception_fallback() {
    assert_eq!(
        run_php(
            "<?php
class Accumulator {
    public $total = 0;
    public function add($value) {
        $this->total = $this->total + $value;
    }
}
function fractional() { return 2.5; }
function failValue() { throw new Exception('stop'); }
$accumulator = new Accumulator();
$accumulator->add(0);
$accumulator->add(fractional());
try {
    $accumulator->add(failValue());
} catch (Exception $exception) {
    $accumulator->add(3);
}
echo $accumulator->total;
"
        ),
        "5.5"
    );
}

#[test]
fn test_composed_property_call_preserves_same_object_aliasing() {
    assert_eq!(
        run_php(
            "<?php
class Counter {
    public $value = 1;
    public function value() { return $this->value; }
    public function add($amount) {
        $this->value = $this->value + $amount;
    }
}
$counter = new Counter();
$counter->add($counter->value());
$counter->add($counter->value());
echo $counter->value;
"
        ),
        "4"
    );
}

#[test]
fn test_composed_property_call_overflow_fallback_is_transactional() {
    assert_eq!(
        run_php(
            "<?php
class Source {
    public $value = 1;
    public function value() { return $this->value; }
}
class Counter {
    public $large = 9223372036854775807;
    public $calls = 0;
    public function update($value) {
        $this->calls = $this->calls + 1;
        $this->large = $this->large + $value;
    }
}
$source = new Source();
$counter = new Counter();
$counter->update($source->value());
$counter->update($source->value());
echo $counter->calls;
"
        ),
        "2"
    );
}

#[test]
fn test_hot_general_comparison_results() {
    // General CV/CV comparisons materialize a scalar boolean when there is no
    // immediately fusible conditional jump.
    assert_eq!(
        run_php(
            "<?php
function less($a, $b) { return $a < $b; }
function less_equal($a, $b) { return $a <= $b; }
function equal($a, $b) { return $a == $b; }
function not_equal($a, $b) { return $a != $b; }

$lt = false;
$le = false;
$eq = false;
$ne = false;
for ($i = 0; $i < 20; $i++) {
    $lt = less(1, 2);
    $le = less_equal(2, 2);
    $eq = equal(3, 3);
    $ne = not_equal(4, 5);
}
echo ($lt ? '1' : '0') . '|' . ($le ? '1' : '0') . '|' .
    ($eq ? '1' : '0') . '|' . ($ne ? '1' : '0');
"
        ),
        "1|1|1|1"
    );
}
