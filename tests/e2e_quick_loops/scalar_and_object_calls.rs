#[test]
fn quick_long_loop_accumulates_pure_scalar_function_result() {
    assert_eq!(
        run_php(
            "<?php
function affine($value, $scale, $bias) {
    return $value * $scale + $bias;
}
$scale = 2;
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += affine($i, $scale, 1);
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "1000000|1000"
    );
}

#[test]
fn quick_scalar_call_trace_guard_replays_taken_cold_edge() {
    assert_eq!(
        run_php(
            "<?php
function identity(int $value): int {
    return $value;
}
$needle = 73;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += identity($i);
    if ($i === $needle) {
        echo 'hit:' . $i . '|';
    }
}
echo $sum . '|' . $i;
"
        ),
        "hit:73|4950|100"
    );
}

#[test]
fn quick_scalar_call_evaluates_checked_argument_expression() {
    assert_eq!(
        run_php(
            "<?php
function combine($left, $right) {
    return $left + $right;
}
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += combine($i, $i + 1);
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "1000000|1000"
    );
}

#[test]
fn quick_scalar_call_executes_small_fused_program_sizes() {
    assert_eq!(
        run_php(
            "<?php
function identity($value) {
    return $value;
}
function add_one($value) {
    return $value + 1;
}
function three_ops($value) {
    return (($value + 1) * 2) - 3;
}
function four_ops($value) {
    return ((($value + 1) * 2) - 3) + 4;
}
$identity_sum = 0;
$one_sum = 0;
$three_sum = 0;
$four_sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $identity_sum += identity($i);
}
for ($i = 0; $i < 1000; $i++) {
    $one_sum += add_one($i);
}
for ($i = 0; $i < 1000; $i++) {
    $three_sum += three_ops($i);
}
for ($i = 0; $i < 1000; $i++) {
    $four_sum += four_ops($i);
}
echo $identity_sum;
echo '|';
echo $one_sum;
echo '|';
echo $three_sum;
echo '|';
echo $four_sum;
"
        ),
        "499500|500500|998000|1002000"
    );
}

#[test]
fn quick_scalar_argument_overflow_restarts_call_transactionally() {
    assert_eq!(
        run_php(
            "<?php
function normalize($value, $offset) {
    return $value - $offset;
}
$offset = PHP_INT_MAX - 64;
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += normalize($i + $offset, $offset);
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $i;
"
        ),
        "float|1000"
    );
}

#[test]
fn quick_scalar_call_executes_composed_function_body() {
    assert_eq!(
        run_php(
            "<?php
function add1($value) {
    return $value + 1;
}
function double($value) {
    return $value + $value;
}
function combine($left, $right) {
    return add1($left) + double($right);
}
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += combine($i, $i + 1);
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "1501500|1000"
    );
}

#[test]
fn quick_typed_function_elides_composed_method_receiver_frame() {
    assert_eq!(
        run_php(
            "<?php
class Source {
    public function value(int $value): int {
        if (($value & 1) === 0) {
            return $value + 3;
        }
        return $value - 2;
    }
}
function consume(Source $source, int $value): int {
    $local = $source->value($value);
    return ($local % 97) ^ 13;
}
function total($source) {
    $sum = 0;
    for ($i = 0; $i < 1000; $i++) {
        $sum += consume($source, $i);
    }
    return $sum . '|' . $i;
}
echo total(new Source());
"
        ),
        "47110|1000"
    );
}

#[test]
fn quick_typed_composed_method_receiver_rechecks_class_contract() {
    assert_eq!(
        run_php(
            "<?php
class Source {
    public function value(int $value): int { return $value + 1; }
}
class Other {
    public function value(int $value): int { return $value + 100; }
}
function consume(Source $source, int $value): int {
    return $source->value($value) + 1;
}
function total($source) {
    $sum = 0;
    for ($i = 0; $i < 1000; $i++) {
        $sum += consume($source, $i);
    }
    return $sum;
}
echo total(new Source()) . '|';
try {
    total(new Other());
} catch (TypeError $error) {
    echo 'type-error';
}
"
        ),
        "501500|type-error"
    );
}

#[test]
fn quick_typed_composed_method_receiver_preserves_impure_fallback() {
    assert_eq!(
        run_php(
            "<?php
$calls = 0;
class ObservedSource {
    public function value(int $value): int {
        global $calls;
        $calls++;
        return $value;
    }
}
function consume(ObservedSource $source, int $value): int {
    return $source->value($value) + 1;
}
$source = new ObservedSource();
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += consume($source, $i);
}
echo $sum . '|' . $calls . '|' . $i;
"
        ),
        "500500|1000|1000"
    );
}

#[test]
fn quick_typed_string_return_is_borrowed_by_length_consumer() {
    assert_eq!(
        run_php(
            "<?php
function label(int $value): string {
    if (($value & 1) === 0) {
        return 'typed-even';
    }
    return 'typed-odd';
}
function consume(int $value): int {
    $label = label($value);
    return strlen($label) + strlen($label) + strlen($label) + strlen($label)
        + strlen($label) + strlen($label) + strlen($label) + strlen($label);
}
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += consume($i);
}
echo $sum . '|' . $i;
"
        ),
        "76000|1000"
    );
}

#[test]
fn quick_typed_string_return_preserves_impure_fallback() {
    assert_eq!(
        run_php(
            "<?php
$calls = 0;
function observedLabel(int $value): string {
    global $calls;
    $calls++;
    return (($value & 1) === 0) ? 'even' : 'odd';
}
function consumeObserved(int $value): int {
    $label = observedLabel($value);
    return strlen($label) + strlen($label);
}
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += consumeObserved($i);
}
echo $sum . '|' . $calls . '|' . $i;
"
        ),
        "7000|1000|1000"
    );
}

#[test]
fn quick_typed_borrowed_string_concat_feeds_strlen() {
    assert_eq!(
        run_php(
            "<?php
function label(int $value): string {
    if (($value & 1) === 0) {
        return 'even';
    }
    return 'odd';
}
function consume(int $value): int {
    return strlen(label($value) . '!');
}
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += consume($i);
}
echo $sum . '|' . $i;
"
        ),
        "4500|1000"
    );
}

#[test]
fn quick_composed_call_guard_preserves_nested_side_effects() {
    assert_eq!(
        run_php(
            "<?php
$calls = 0;
function observed_helper($value) {
    global $calls;
    $calls++;
    return $value;
}
function observed_root($value) {
    return observed_helper($value) + 1;
}
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += observed_root($i);
}
echo $sum;
echo '|';
echo $calls;
echo '|';
echo $i;
"
        ),
        "500500|1000|1000"
    );
}

#[test]
fn quick_scalar_method_executes_nested_monomorphic_call_tree() {
    assert_eq!(
        run_php(
            "<?php
class Math {
    public function add($left, $right) { return $left + $right; }
    public function mul($left, $right) { return $left * $right; }
}
$math = new Math();
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $math->add($i, $math->mul($i, 2));
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "1498500|1000"
    );
}

#[test]
fn quick_scalar_method_accepts_checked_argument_expression() {
    assert_eq!(
        run_php(
            "<?php
class Math {
    public function add($left, $right) { return $left + $right; }
}
$math = new Math();
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $math->add($i, $i + 1);
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "1000000|1000"
    );
}

#[test]
fn quick_scalar_method_cache_tracks_receiver_class_between_activations() {
    assert_eq!(
        run_php(
            "<?php
class IdentityMath {
    public function apply($value) { return $value; }
}
class DoubleMath {
    public function apply($value) { return $value * 2; }
}
function total($math) {
    $sum = 0;
    for ($i = 0; $i < 1000; $i++) {
        $sum += $math->apply($i);
    }
    return $sum;
}
echo total(new IdentityMath());
echo '|';
echo total(new DoubleMath());
"
        ),
        "499500|999000"
    );
}

#[test]
fn quick_scalar_method_reference_receiver_uses_canonical_fallback() {
    assert_eq!(
        run_php(
            "<?php
class Math {
    public function apply($value) { return $value + 1; }
}
function total(&$math) {
    $sum = 0;
    for ($i = 0; $i < 1000; $i++) {
        $sum += $math->apply($i);
    }
    return $sum . '|' . $i;
}
$math = new Math();
echo total($math);
"
        ),
        "500500|1000"
    );
}

#[test]
fn quick_scalar_method_guard_preserves_impure_side_effects() {
    assert_eq!(
        run_php(
            "<?php
$calls = 0;
class ObservedMath {
    public function apply($value) {
        global $calls;
        $calls++;
        return $value;
    }
}
$math = new ObservedMath();
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $math->apply($i);
}
echo $sum;
echo '|';
echo $calls;
echo '|';
echo $i;
"
        ),
        "499500|1000|1000"
    );
}

#[test]
fn quick_object_loop_executes_property_calls_and_conditional_composition() {
    assert_eq!(
        run_php(
            "<?php
class Tick {
    public $value = 0;
    public function advance() { $this->value = $this->value + 1; }
    public function current() { return $this->value; }
}
class Sink {
    public $value = 0;
    public function accept($value) { $this->value = $this->value + $value; }
    public function result() { return $this->value; }
}
$tick = new Tick();
$sink = new Sink();
for ($i = 0; $i < 1000; $i++) {
    $tick->advance();
    if ($i % 3 == 0) {
        $sink->accept($tick->current());
    }
}
echo $tick->current() . '|' . $sink->result() . '|' . $i;
"
        ),
        "1000|167167|1000"
    );
}

#[test]
fn quick_object_loop_guards_receiver_class_between_activations() {
    assert_eq!(
        run_php(
            "<?php
class StepOne {
    public $value = 0;
    public function advance() { $this->value = $this->value + 1; }
    public function current() { return $this->value; }
}
class StepTwo {
    public $value = 0;
    public function advance() { $this->value = $this->value + 2; }
    public function current() { return $this->value; }
}
class Sink {
    public $value = 0;
    public function accept($value) { $this->value = $this->value + $value; }
    public function result() { return $this->value; }
}
function collect($step) {
    $sink = new Sink();
    for ($i = 0; $i < 100; $i++) {
        $step->advance();
        if ($i % 4 == 0) {
            $sink->accept($step->current());
        }
    }
    return $step->current() . ':' . $sink->result();
}
echo collect(new StepOne()) . '|' . collect(new StepTwo());
"
        ),
        "100:1225|200:2450"
    );
}

#[test]
fn quick_object_loop_property_overflow_deoptimizes_transactionally() {
    assert_eq!(
        run_php(
            "<?php
class Tick {
    public $value = 9223372036854775767;
    public function advance() { $this->value = $this->value + 1; }
    public function current() { return $this->value; }
}
$tick = new Tick();
for ($i = 0; $i < 100; $i++) {
    $tick->advance();
}
echo gettype($tick->current()) . '|' . $i;
"
        ),
        "double|100"
    );
}

#[test]
fn quick_object_loop_executes_separate_mutator_and_getter_ops() {
    assert_eq!(
        run_php(
            "<?php
class Counter {
    public $value = 0;
    public function add($amount) { $this->value = $this->value + $amount; }
    public function current() { return $this->value; }
}
$counter = new Counter();
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $counter->add(2);
    $current = $counter->current();
    $sum += $current;
}
echo $counter->current() . '|' . $sum . '|' . $i;
"
        ),
        "2000|1001000|1000"
    );
}

#[test]
fn quick_object_loop_preserves_getter_observation_through_receiver_alias() {
    assert_eq!(
        run_php(
            "<?php
class AliasedCounter {
    public $value = 0;
    public function add($amount) { $this->value = $this->value + $amount; }
    public function current() { return $this->value; }
}
$counter = new AliasedCounter();
$observer = $counter;
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $counter->add(2);
    $sum += $observer->current();
}
echo $counter->current() . '|' . $sum . '|' . $i;
"
        ),
        "2000|1001000|1000"
    );
}

#[test]
fn quick_object_loop_commits_multi_property_shadows_before_later_side_exit() {
    assert_eq!(
        run_php(
            "<?php
class ShadowedPair {
    public $left = 0;
    public $right = 0;
    public function add($left, $right) {
        $this->left = $this->left + $left;
        $this->right = $this->right + $right;
    }
}
$pair = new ShadowedPair();
for ($i = 0; $i < 100; $i++) {
    $pair->add(1, 2);
    $overflow = 9223372036854775800 + $i;
}
echo $pair->left . '|' . $pair->right . '|' . gettype($overflow) . '|' . $i;
"
        ),
        "100|200|double|100"
    );
}

#[test]
fn quick_object_loop_rejects_impure_property_method_before_side_effects() {
    assert_eq!(
        run_php(
            "<?php
$calls = 0;
class ObservedCounter {
    public $value = 0;
    public function advance() {
        global $calls;
        $calls++;
        $this->value = $this->value + 1;
    }
    public function current() { return $this->value; }
}
$counter = new ObservedCounter();
for ($i = 0; $i < 1000; $i++) {
    $counter->advance();
}
echo $counter->current() . '|' . $calls . '|' . $i;
"
        ),
        "1000|1000|1000"
    );
}

#[test]
fn quick_scalar_call_guard_preserves_impure_function_side_effects() {
    assert_eq!(
        run_php(
            "<?php
$calls = 0;
function observed($value) {
    global $calls;
    $calls++;
    return $value;
}
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += observed($i);
}
echo $sum;
echo '|';
echo $calls;
echo '|';
echo $i;
"
        ),
        "499500|1000|1000"
    );
}

#[test]
fn quick_scalar_call_deoptimizes_accumulator_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
function identity($value) {
    return $value;
}
$sum = PHP_INT_MAX - 100000;
for ($i = 0; $i < 1000; $i++) {
    $sum += identity($i);
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $i;
"
        ),
        "float|1000"
    );
}

#[test]
fn quick_abs_guard_falls_back_for_double_value() {
    assert_eq!(
        run_php(
            "<?php
$value = -1.5;
$sum = 0;
for ($i = 0; $i < 1000; ++$i) {
    $sum += abs($value);
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $sum;
echo '|';
echo $i;
"
        ),
        "float|1500|1000"
    );
}

#[test]
fn quick_abs_deoptimizes_accumulator_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$value = -7;
$sum = PHP_INT_MAX - 350;
for ($i = 0; $i < 100; ++$i) {
    $sum += abs($value);
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $i;
"
        ),
        "float|100"
    );
}
