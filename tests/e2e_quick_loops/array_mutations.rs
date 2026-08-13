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
fn quick_long_array_push_preserves_preexisting_dense_prefix() {
    assert_eq!(
        run_php(
            "<?php
$values = [10, 20];
for ($i = 0; $i < 70000; $i++) {
    $values[] = $i;
}
echo count($values) . '|' . $values[0] . '|' . $values[1] . '|';
echo $values[2] . '|' . $values[70001];
"
        ),
        "70002|10|20|0|69999"
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
fn packed_array_read_and_structural_push_uses_stable_fallback() {
    assert_eq!(
        run_php(
            "<?php
$values = [3];
$sum = 0;
for ($i = 0; $i < 1000; $i++) {
    $sum += $values[0];
    $values[] = $i;
}
echo $sum . '|' . count($values) . '|' . $values[1000];
"
        ),
        "3000|1001|999"
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
fn quick_structural_integer_array_set_preserves_updates_order_and_cow() {
    assert_eq!(
        run_php(
            "<?php
$values = [1000 => -1];
$copy = $values;
for ($i = 0; $i < 1000; $i++) {
    $key = (($i * 17) & 255) + 1000;
    $values[$key] = $i;
}
$first = '';
$seen = 0;
foreach ($values as $key => $value) {
    if ($seen < 4) {
        $first .= $key . ':' . $value . ',';
    }
    $seen++;
}
echo count($values) . '|' . $values[1000] . '|' . $values[1017] . '|';
echo count($copy) . '|' . $copy[1000] . '|' . $first;
"
        ),
        "256|768|769|1|-1|1000:768,1017:769,1034:770,1051:771,"
    );
}

#[test]
fn quick_structural_integer_array_set_preserves_shift_keys() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
for ($i = 0; $i < 1000; $i++) {
    $key = ($i << 32) | (($i * $i) & 1048575);
    $values[$key] = $i;
}
$left = 1;
$right = -256;
for ($j = 0; $j < 100; $j++) {
    $left = $left << 65;
    $right = $right >> 65;
}
echo count($values) . '|' . $values[$key] . '|' . $key . '|';
echo $left . '|' . $right;
"
        ),
        "1000|999|4290673326705|0|-1"
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
