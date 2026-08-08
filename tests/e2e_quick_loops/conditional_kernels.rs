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
