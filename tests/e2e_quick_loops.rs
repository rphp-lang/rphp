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
