mod common;

use common::run_php;

#[test]
fn quick_long_loop_in_main_keeps_exact_result() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $sum += $i + 1;
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "500500|1000"
    );
}

#[test]
fn quick_long_loop_works_inside_user_function() {
    assert_eq!(
        run_php(
            "<?php
function total($n) {
    $sum = 0;
    for ($i = 0; $i < $n; $i++) {
        $sum += $i + 1;
    }
    return $sum;
}
echo total(1000);
"
        ),
        "500500"
    );
}

#[test]
fn quick_long_loop_survives_hot_function_bailout() {
    assert_eq!(
        run_php(
            "<?php
function total($n) {
    $sum = 0;
    for ($i = 0; $i < $n; $i++) {
        $sum += $i + 1;
    }
    return $sum;
}
$result = 0;
for ($call = 0; $call < 20; $call++) {
    $result = total(100);
}
echo $result;
"
        ),
        "5050"
    );
}

#[test]
fn quick_long_loop_supports_other_integer_addends() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $sum += $i + 2;
}
echo $sum;
"
        ),
        "501500"
    );
}

#[test]
fn quick_long_loop_accumulates_induction_directly() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $sum += $i;
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "499500|1000"
    );
}

#[test]
fn quick_long_loop_accumulates_invariant_string_length() {
    assert_eq!(
        run_php(
            "<?php
$string = 'abcd';
$sum = 0;
for ($i = 0; $i < 1000; ++$i) {
    $sum += strlen($string);
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "4000|1000"
    );

    assert_eq!(
        run_php(
            "<?php
$string = 'ž';
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += strlen($string);
}
echo $sum;
"
        ),
        "2000"
    );
}

#[test]
fn quick_string_length_guard_falls_back_for_non_string_value() {
    assert_eq!(
        run_php(
            "<?php
$value = 1234;
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += strlen($value);
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "4000|1000"
    );
}

#[test]
fn quick_string_length_deoptimizes_accumulator_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$string = 'abcd';
$sum = PHP_INT_MAX - 200;
for ($i = 0; $i < 100; ++$i) {
    $sum += strlen($string);
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $i;
"
        ),
        "float|100"
    );
}

#[test]
fn quick_long_loop_accumulates_invariant_abs() {
    assert_eq!(
        run_php(
            "<?php
$value = -7;
$sum = 0;
for ($i = 0; $i < 1000; ++$i) {
    $sum += abs($value);
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "7000|1000"
    );
}

#[test]
fn quick_long_loop_computes_abs_of_induction() {
    assert_eq!(
        run_php(
            "<?php
$sum = 0;
for ($i = -1000; $i < 1000; ++$i) {
    $sum += abs($i);
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

#[test]
fn quick_long_loop_supports_while_shape() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = 0;
$i = 0;
while ($i < $n) {
    $sum += $i;
    $i++;
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "499500|1000"
    );
}

#[test]
fn quick_induction_only_for_loop_supports_prefix_increment() {
    assert_eq!(
        run_php(
            "<?php
for ($i = 0; $i < 10000; ++$i) {
}
echo $i;
"
        ),
        "10000"
    );
}

#[test]
fn quick_induction_only_while_loop_supports_postincrement() {
    assert_eq!(
        run_php(
            "<?php
$limit = 10000;
$i = 0;
while ($i < $limit) {
    $i++;
}
echo $i;
"
        ),
        "10000"
    );
}

#[test]
fn quick_induction_only_guard_falls_back_for_double_bound() {
    assert_eq!(
        run_php(
            "<?php
$limit = 1000.5;
$i = 0;
while ($i < $limit) {
    $i++;
}
echo $i;
"
        ),
        "1001"
    );
}

#[test]
fn quick_branch_only_if_else_loop_finishes_exactly() {
    assert_eq!(
        run_php(
            "<?php
for ($i = 0; $i < 10000; $i++) {
    if ($i == -1) {
    } elseif ($i == -2) {
    } else if ($i == -3) {
    }
}
echo $i;
"
        ),
        "10000"
    );
}

#[test]
fn quick_long_loop_supports_fused_constant_bound() {
    assert_eq!(
        run_php(
            "<?php
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $i + 1;
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "500500|1000"
    );
}

#[test]
fn quick_long_loop_deoptimizes_at_accumulator_overflow() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = PHP_INT_MAX - 100000;
for ($i = 0; $i < $n; $i++) {
    $sum += $i + 1;
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
fn quick_direct_accumulation_deoptimizes_at_overflow() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = PHP_INT_MAX - 100000;
for ($i = 0; $i < $n; $i++) {
    $sum += $i;
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
fn quick_long_loop_rejects_non_long_bound() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000.0;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $sum += $i + 1;
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "500500|1000"
    );
}

#[test]
fn quick_long_loop_rejects_reference_accumulator() {
    assert_eq!(
        run_php(
            "<?php
$sum = 0;
function total($n) {
    global $sum;
    for ($i = 0; $i < $n; $i++) {
        $sum += $i + 1;
    }
    return $sum;
}
echo total(1000);
echo '|';
echo $sum;
"
        ),
        "500500|500500"
    );
}

#[test]
fn quick_long_ops_support_two_cv_nested_term() {
    assert_eq!(
        run_php(
            "<?php
$sum = 0;
for ($i = 0; $i < 10; $i++) {
    for ($j = 0; $j < 20; $j++) {
        $sum += $i + $j;
    }
}
echo $sum;
echo '|';
echo $i;
echo '|';
echo $j;
"
        ),
        "2800|10|20"
    );
}

#[test]
fn quick_accumulate_supports_commuted_invariant_cv_term() {
    assert_eq!(
        run_php(
            "<?php
$offset = 7;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $offset + $i;
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "5650|100"
    );
}

#[test]
fn quick_accumulate_deoptimizes_at_invariant_cv_term_overflow() {
    assert_eq!(
        run_php(
            "<?php
$offset = PHP_INT_MAX;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $i + $offset;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $i;
"
        ),
        "float|100"
    );
}

#[test]
fn quick_long_ops_support_conditional_body() {
    assert_eq!(
        run_php(
            "<?php
$n = 100;
$cutoff = 50;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $sum += $i;
    }
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "1225|100"
    );
}

#[test]
fn quick_long_ops_preserve_never_written_tmp() {
    assert_eq!(
        run_php(
            "<?php
$n = 100;
$cutoff = 0;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $sum += $i;
    }
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "0|100"
    );
}

#[test]
fn quick_less_than_conditional_kernel_deoptimizes_at_overflow() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$cutoff = 1000;
$sum = PHP_INT_MAX - 100000;
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $sum += $i;
    }
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
fn quick_conditional_kernel_falls_back_for_direct_equality() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$needle = 500;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if ($i == $needle) {
        $sum += $i;
    }
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "500|1000"
    );
}

#[test]
fn quick_long_ops_deoptimize_nested_accumulator_overflow() {
    assert_eq!(
        run_php(
            "<?php
$sum = PHP_INT_MAX - 100000;
for ($i = 0; $i < 1; $i++) {
    for ($j = 0; $j < 1000; $j++) {
        $sum += $i + $j;
    }
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $i;
echo '|';
echo $j;
"
        ),
        "float|1|1000"
    );
}

#[test]
fn quick_long_ops_deoptimize_add_assign_overflow() {
    assert_eq!(
        run_php(
            "<?php
$base = PHP_INT_MAX - 40;
$result = 0;
for ($i = 0; $i < 100; $i++) {
    $result = $base + $i;
}
echo is_float($result) ? 'float' : 'int';
echo '|';
echo $i;
"
        ),
        "float|100"
    );
}

#[test]
fn quick_long_ops_reject_non_long_internal_branch_bound() {
    assert_eq!(
        run_php(
            "<?php
$n = 100;
$cutoff = 50.0;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $sum += $i;
    }
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "1225|100"
    );
}

#[test]
fn quick_long_ops_support_modulo_equality_branch() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if (($i % 3) == 1) {
        $sum += $i;
    }
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "166167|1000"
    );
}

#[test]
fn quick_long_ops_modulo_matches_negative_remainder_semantics() {
    assert_eq!(
        run_php(
            "<?php
$sum = 0;
for ($i = -100; $i < 100; $i++) {
    if (($i % 2) == 0) {
        $sum += $i;
    }
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "-100|100"
    );
}

#[test]
fn quick_conditional_kernel_falls_back_for_modulo_less_than() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if (($i % 3) < 2) {
        $sum += $i;
    }
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "333000|1000"
    );
}

#[test]
fn quick_long_ops_read_packed_long_array() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 1000);
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$i];
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "500500|1000"
    );
}

#[test]
fn quick_packed_array_read_deoptimizes_for_missing_key() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$sum = 0;
$last = 0;
for ($i = 0; $i < 200; $i++) {
    $last = $values[$i];
    $sum += $i;
}
echo $sum;
echo '|';
echo is_null($last) ? 'null' : 'value';
echo '|';
echo $i;
"
        ),
        "19900|null|200"
    );
}

#[test]
fn quick_packed_array_read_deoptimizes_for_non_long_value() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$values[99] = 'marker';
$sum = 0;
$last = 0;
for ($i = 0; $i < 100; $i++) {
    $last = $values[$i];
    $sum += $i;
}
echo $sum;
echo '|';
echo $last;
echo '|';
echo $i;
"
        ),
        "4950|marker|100"
    );
}

#[test]
fn quick_array_read_survives_transition_to_hash_storage() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$values['extra'] = 999;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$i];
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "5050|100"
    );
}

#[test]
fn quick_hash_array_reads_integer_keys() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 1000);
$values['sentinel'] = 0;
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$i];
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "500500|1000"
    );
}

#[test]
fn quick_hash_array_reads_string_literal_key() {
    assert_eq!(
        run_php(
            "<?php
$values = ['hot' => 7];
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values['hot'];
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "7000|1000"
    );
}

#[test]
fn quick_hash_array_reads_invariant_string_value_slot() {
    assert_eq!(
        run_php(
            "<?php
$values = ['hot' => 7];
$key = 'hot';
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$key];
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "7000|hot|1000"
    );
}

#[test]
fn quick_hash_array_materializes_invariant_string_value_slot() {
    assert_eq!(
        run_php(
            "<?php
$values = ['hot' => 7];
$key = 'hot';
$sum = 0;
$value = 0;
for ($i = 0; $i < 1000; $i++) {
    $value = $values[$key];
    $sum += $value;
}
echo $sum;
echo '|';
echo $value;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "7000|7|hot|1000"
    );
}

#[test]
fn quick_hash_array_normalizes_invariant_numeric_string_value_slot() {
    assert_eq!(
        run_php(
            "<?php
$values = [7 => 9, 'sentinel' => 0];
$key = '7';
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$key];
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "9000|7|1000"
    );
}

#[test]
fn quick_hash_array_tracks_dynamic_string_key_state() {
    assert_eq!(
        run_php(
            "<?php
$values = ['left' => 3, 'right' => 5];
$key = 'left';
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = 'right';
    } else {
        $key = 'left';
    }
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4000|left|1000"
    );
}

#[test]
fn quick_hash_array_tracks_dynamic_numeric_string_keys() {
    assert_eq!(
        run_php(
            "<?php
$values = [7 => 3, 8 => 5, 'sentinel' => 0];
$key = '7';
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = '8';
    } else {
        $key = '7';
    }
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4000|7|1000"
    );
}

#[test]
fn quick_dynamic_string_key_deoptimizes_non_long_fetch_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = ['left' => 1, 'right' => 'marker'];
$key = 'left';
$last = 0;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $last = $values[$key];
    $sum += $i;
    if ($i == 98) {
        $key = 'right';
    } else {
        $key = 'left';
    }
}
echo $sum;
echo '|';
echo $last;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4950|marker|left|100"
    );
}

#[test]
fn quick_hash_array_tracks_string_key_selected_from_cvs() {
    assert_eq!(
        run_php(
            "<?php
$values = ['left' => 3, 'right' => 5];
$left = 'left';
$right = 'right';
$key = $left;
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = $right;
    } else {
        $key = $left;
    }
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4000|left|1000"
    );
}

#[test]
fn quick_hash_array_normalizes_numeric_string_key_sources() {
    assert_eq!(
        run_php(
            "<?php
$values = [7 => 3, 8 => 5, 'sentinel' => 0];
$left = '7';
$right = '8';
$key = $left;
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = $right;
    } else {
        $key = $left;
    }
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4000|7|1000"
    );
}

#[test]
fn quick_hash_array_string_read_works_in_general_typed_loop() {
    assert_eq!(
        run_php(
            "<?php
$values = ['hot' => 7];
$sum = 0;
$last = 0;
for ($i = 0; $i < 1000; $i++) {
    $last = $values['hot'];
    $sum += $i;
}
echo $sum;
echo '|';
echo $last;
echo '|';
echo $i;
"
        ),
        "499500|7|1000"
    );
}

#[test]
fn quick_long_array_push_builds_unique_packed_array() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$constants = [];
for ($i = 0; $i < 1000; $i++) {
    $values[] = $i;
}
for ($i = 0; $i < 1000; $i++) {
    $constants[] = 7;
}
echo count($values);
echo '|';
echo $values[0];
echo '|';
echo $values[999];
echo '|';
echo $constants[999];
echo '|';
echo $i;
"
        ),
        "1000|0|999|7|1000"
    );
}

#[test]
fn quick_long_array_push_preserves_cow_alias() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$copy = $values;
for ($i = 0; $i < 1000; $i++) {
    $values[] = $i;
}
echo count($values);
echo '|';
echo count($copy);
echo '|';
echo $values[999];
"
        ),
        "1000|0|999"
    );
}

#[test]
fn quick_long_array_push_reference_uses_canonical_fallback() {
    assert_eq!(
        run_php(
            "<?php
function append_many(&$values) {
    for ($i = 0; $i < 100; $i++) {
        $values[] = 7;
    }
}
$values = [];
append_many($values);
echo count($values);
echo '|';
echo $values[99];
"
        ),
        "100|7"
    );
}

#[test]
fn quick_string_append_handles_literal_and_invariant_sources() {
    assert_eq!(
        run_php(
            "<?php
$literal_result = '';
for ($i = 0; $i < 1000; $i++) {
    $literal_result .= 'x';
}
$suffix = 'yz';
$invariant_result = '';
for ($i = 0; $i < 1000; $i++) {
    $invariant_result .= $suffix;
}
echo strlen($literal_result);
echo '|';
echo strlen($invariant_result);
echo '|';
echo $suffix;
echo '|';
echo $i;
"
        ),
        "1000|2000|yz|1000"
    );
}

#[test]
fn quick_string_append_preserves_cow_alias() {
    assert_eq!(
        run_php(
            "<?php
$value = 'base';
$copy = $value;
for ($i = 0; $i < 1000; $i++) {
    $value .= 'x';
}
echo strlen($value);
echo '|';
echo $copy;
"
        ),
        "1004|base"
    );
}

#[test]
fn quick_string_append_type_and_reference_guards_use_canonical_fallback() {
    assert_eq!(
        run_php(
            "<?php
function append_by_reference(&$value) {
    for ($i = 0; $i < 100; $i++) {
        $value .= 'x';
    }
}
$referenced = '';
append_by_reference($referenced);
$numeric_suffix = 7;
$converted = '';
for ($i = 0; $i < 100; $i++) {
    $converted .= $numeric_suffix;
}
$self = 'x';
for ($i = 0; $i < 5; $i++) {
    $self .= $self;
}
echo strlen($referenced);
echo '|';
echo strlen($converted);
echo '|';
echo strlen($self);
"
        ),
        "100|100|32"
    );
}

#[test]
fn quick_hash_invariant_string_fetch_falls_back_for_missing_value_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$sum = 0;
$last = 7;
for ($i = 0; $i < 100; $i++) {
    $last = $values['missing'];
    $sum += $i;
}
echo $sum;
echo '|';
echo is_null($last) ? 'null' : 'value';
echo '|';
echo $i;
"
        ),
        "4950|null|100"
    );
}

#[test]
fn quick_hash_invariant_string_fetch_falls_back_for_non_long_value_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = ['hot' => 'marker'];
$sum = 0;
$last = 7;
for ($i = 0; $i < 100; $i++) {
    $last = $values['hot'];
    $sum += $i;
}
echo $sum;
echo '|';
echo $last;
echo '|';
echo $i;
"
        ),
        "4950|marker|100"
    );
}

#[test]
fn quick_hash_invariant_integer_fetch_materializes_value_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = [7 => 9, 'sentinel' => 0];
$sum = 0;
$value = 0;
for ($i = 0; $i < 1000; $i++) {
    $value = $values[7];
    $sum += $value;
}
echo $sum;
echo '|';
echo $value;
echo '|';
echo $i;
"
        ),
        "9000|9|1000"
    );
}

#[test]
fn quick_hash_array_normalizes_numeric_string_literal_key() {
    assert_eq!(
        run_php(
            "<?php
$values = [7 => 9, 'sentinel' => 0];
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values['7'];
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "9000|1000"
    );
}

#[test]
fn quick_hash_integer_read_deoptimizes_for_missing_key() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$values['sentinel'] = 0;
$sum = 0;
$last = 0;
for ($i = 0; $i < 200; $i++) {
    $last = $values[$i];
    $sum += $i;
}
echo $sum;
echo '|';
echo is_null($last) ? 'null' : 'value';
echo '|';
echo $i;
"
        ),
        "19900|null|200"
    );
}

#[test]
fn quick_hash_integer_read_deoptimizes_for_non_long_value() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$values['sentinel'] = 0;
$values[99] = 'marker';
$sum = 0;
$last = 0;
for ($i = 0; $i < 100; $i++) {
    $last = $values[$i];
    $sum += $i;
}
echo $sum;
echo '|';
echo $last;
echo '|';
echo $i;
"
        ),
        "4950|marker|100"
    );
}

#[test]
fn quick_hash_one_add_kernel_deoptimizes_accumulator_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$values['sentinel'] = 0;
$sum = PHP_INT_MAX - 2000;
for ($i = 0; $i < 100; $i++) {
    $value = $values[$i];
    $sum += $value;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $value;
echo '|';
echo $i;
"
        ),
        "float|100|100"
    );
}

#[test]
fn quick_hash_string_materialized_accumulate_deoptimizes_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = ['hot' => 100];
$sum = PHP_INT_MAX - 4000;
for ($i = 0; $i < 100; $i++) {
    $value = $values['hot'];
    $sum += $value;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $value;
echo '|';
echo $i;
"
        ),
        "float|100|100"
    );
}

#[test]
fn quick_hash_stride_kernel_updates_key_and_accumulator() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 1000; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$key = $start;
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[$key];
    $key = $key + $stride;
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "499500|7100|1000"
    );
}

#[test]
fn quick_hash_stride_kernel_deoptimizes_non_long_fetch_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$values[793] = 1.5;
$key = $start;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    $key = $key + $stride;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "float|800|100"
    );
}

#[test]
fn quick_hash_stride_kernel_deoptimizes_accumulator_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 100; $i++) {
    $values[$key] = 1;
    $key = $key + $stride;
}
$key = $start;
$sum = PHP_INT_MAX - 40;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    $key = $key + $stride;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "float|800|100"
    );
}

#[test]
fn quick_hash_position_hint_supports_negative_stride() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 1000;
$stride = -7;
$key = $start;
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$key = $start;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    $key = $key + $stride;
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4950|300|100"
    );
}

#[test]
fn quick_hash_position_hint_routes_suffix_and_falls_back_before_separator() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
foreach ([11, 30, 31, 70, -4, 900, 2, 88] as $prefix) {
    $values[$prefix] = -1;
}
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 100; $i++) {
    if ($i == 50) {
        $values['separator'] = 0;
    }
    $values[$key] = $i;
    $key = $key + $stride;
}
$one = 1;
$key = $start;
$sum = 0;
$adjusted = 0;
for ($i = 0; $i < 100; $i++) {
    $value = $values[$key];
    $sum += $value;
    $adjusted += $value + $one;
    $key = $key + $stride;
}
echo $sum;
echo '|';
echo $adjusted;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4950|5050|800|100"
    );
}

#[test]
fn quick_hash_position_hint_falls_back_after_reordered_entry() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
unset($values[450]);
$values[450] = 50;
$key = $start;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    $key = $key + $stride;
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4950|800|100"
    );
}

#[test]
fn quick_hash_general_program_routes_irregular_integer_reads() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$one = 1;
$key = $start;
$sum = 0;
$adjusted = 0;
for ($i = 0; $i < 100; $i++) {
    $value = $values[$key];
    $sum += $value;
    $adjusted += $value + $one;
    $key = $key + $stride;
}
echo $sum;
echo '|';
echo $adjusted;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4950|5050|800|100"
    );
}

#[test]
fn quick_hash_general_program_deoptimizes_non_long_fetch_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$values[793] = 'marker';
$one = 1;
$key = $start;
$sum = 0;
$adjusted = 0;
$last = 0;
for ($i = 0; $i < 100; $i++) {
    $last = $values[$key];
    $sum += $i;
    $adjusted += $i + $one;
    $key = $key + $stride;
}
echo $sum;
echo '|';
echo $adjusted;
echo '|';
echo $last;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "4950|5050|marker|800|100"
    );
}

#[test]
fn quick_hash_transform_deoptimizes_fused_add_without_replaying_prior_add() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 100; $i++) {
    $values[$key] = 1;
    $key = $key + $stride;
}
$one = 1;
$key = $start;
$sum = 0;
$adjusted = PHP_INT_MAX - 100;
for ($i = 0; $i < 100; $i++) {
    $value = $values[$key];
    $sum += $value;
    $adjusted += $value + $one;
    $key = $key + $stride;
}
echo $sum;
echo '|';
echo is_float($adjusted) ? 'float' : 'int';
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "100|float|800|100"
    );
}

#[test]
fn quick_hash_filtered_program_routes_and_deoptimizes_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
$start = 100;
$stride = 7;
$key = $start;
for ($i = 0; $i < 500; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$cutoff = 500;
$key = $start;
$sum = PHP_INT_MAX - 20000;
for ($i = 0; $i < 500; $i++) {
    $value = $values[$key];
    if ($value < $cutoff) {
        $sum += $value;
    }
    $key = $key + $stride;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $value;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "float|499|3600|500"
    );
}

#[test]
fn quick_conditional_add_assign_deoptimizes_at_overflow() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = PHP_INT_MAX - 100000;
for ($i = 0; $i < $n; $i++) {
    if (($i % 2) == 0) {
        $sum += $i;
    }
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
fn quick_foreach_long_accumulation_finishes_packed_array_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 1000);
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
echo $sum;
echo '|';
echo $value;
"
        ),
        "500500|1000"
    );
}

#[test]
fn quick_foreach_long_accumulation_finishes_hash_array_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 1000);
$values['last'] = 7;
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
echo $sum;
echo '|';
echo $value;
"
        ),
        "500507|7"
    );
}

#[test]
fn quick_foreach_long_accumulation_deoptimizes_non_long_value_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$values[80] = 1.5;
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $sum;
echo '|';
echo $value;
"
        ),
        "float|4970.5|100"
    );
}

#[test]
fn quick_foreach_long_accumulation_deoptimizes_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$sum = PHP_INT_MAX - 1000;
foreach ($values as $value) {
    $sum += $value;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $value;
"
        ),
        "float|100"
    );
}
