#[test]
fn quick_straight_array_region_runs_across_fresh_function_frames() {
    assert_eq!(
        run_php(
            "<?php
function collect($row) {
    $a = 10;
    $b = 20;
    $c = 30;
    $a = $a + $row['a'];
    $b = $b + $row['b'];
    $c = $c + $row['c'];
    return $a + $b + $c;
}
$total = 0;
for ($i = 0; $i < 200; $i++) {
    $total = $total + collect(['a' => 2, 'b' => 3, 'c' => 4]);
}
echo $total;
"
        ),
        "13800"
    );
}

#[test]
fn quick_straight_array_region_missing_value_resumes_current_fetch() {
    assert_eq!(
        run_php(
            "<?php
function collect($row) {
    $a = 10;
    $b = 20;
    $c = 30;
    $a = $a + $row['a'];
    $b = $b + $row['b'];
    $c = $c + $row['c'];
    return $a + $b + $c;
}
$total = 0;
for ($i = 0; $i < 100; $i++) {
    if ($i == 80) {
        $row = ['a' => 2, 'b' => 3.5, 'c' => 4];
    } else {
        $row = ['a' => 2, 'b' => 3, 'c' => 4];
    }
    $total = $total + collect($row);
}
echo $total;
"
        ),
        "6900.5"
    );
}

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
