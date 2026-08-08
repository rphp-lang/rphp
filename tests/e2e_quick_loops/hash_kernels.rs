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
fn quick_hash_composed_bitwise_integer_key_keeps_exact_result() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
}
echo $sum;
echo '|';
echo $key;
echo '|';
echo $i;
"
        ),
        "499500|753618331|1000"
    );
}

#[test]
fn quick_hash_composed_bitwise_key_deoptimizes_non_long_fetch_exactly() {
    assert_eq!(
        run_php(
            "<?php
$n = 100;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$lastKey = ((99 * 1103515245) & 2147483647) + 1000000;
$values[$lastKey] = 1.5;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
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
fn quick_hash_composed_bitwise_key_deoptimizes_accumulator_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$n = 100;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = 1;
}
$sum = PHP_INT_MAX - 40;
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
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
fn quick_hash_composed_key_permutation_falls_back_exactly() {
    assert_eq!(
        run_php(
            "<?php
$n = 1024;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $position = ($i * 271) & 1023;
    $key = (($position * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
}
echo $sum;
echo '|';
echo $i;
"
        ),
        "523776|1024"
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
