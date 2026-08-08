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
fn quick_exact_contiguous_hash_prefix_materializes_integer_reads() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 1000);
$values['sentinel'] = 0;
$sum = 0;
$value = 0;
for ($i = 0; $i < 1000; $i++) {
    $value = $values[$i];
    $sum += $value;
}
echo $sum;
echo '|';
echo $value;
echo '|';
echo $i;
"
        ),
        "500500|1000|1000"
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
fn quick_mixed_region_trace_guard_commits_state_before_taken_cold_edge() {
    assert_eq!(
        run_php(
            "<?php
class MixedGuardModel {
    public function score(int $value, string $key): int {
        return $value + strlen($key);
    }
}
$model = new MixedGuardModel();
$values = ['left' => 0, 'right' => 0];
$key = 'left';
$needle = 73;
for ($i = 0; $i < 100; $i++) {
    if (($i % 2) == 0) {
        $key = 'right';
    } else {
        $key = 'left';
    }
    $score = $model->score($i, $key);
    $values[$key] = $values[$key] + $score;
    if ($i === $needle) {
        echo 'hit:' . $i . '|';
    }
}
echo $values['left'] . ':' . $values['right'] . ':' . $i;
"
        ),
        "hit:73|2700:2700:100"
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
