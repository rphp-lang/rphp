/// E2E tests: foreach loops — value only, key-value, nested, break/continue, edge cases.

mod common;
use common::run_php;

// === Basic foreach ($arr as $val) ===

#[test]
fn test_e2e_foreach_value_only() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; foreach ($a as $v) { echo $v; }"),
        "102030"
    );
}

#[test]
fn test_e2e_foreach_string_values() {
    assert_eq!(
        run_php("<?php $a = ['hello', 'world']; foreach ($a as $v) { echo $v . ' '; }"),
        "hello world "
    );
}

// === foreach ($arr as $key => $val) ===

#[test]
fn test_e2e_foreach_key_value() {
    assert_eq!(
        run_php("<?php $a = ['a' => 1, 'b' => 2, 'c' => 3]; foreach ($a as $k => $v) { echo $k . $v; }"),
        "a1b2c3"
    );
}

#[test]
fn test_e2e_foreach_int_keys() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; foreach ($a as $k => $v) { echo $k . ':' . $v . ' '; }"),
        "0:10 1:20 2:30 "
    );
}

// === Empty array ===

#[test]
fn test_e2e_foreach_empty_array() {
    assert_eq!(
        run_php("<?php $a = []; foreach ($a as $v) { echo $v; } echo 'done';"),
        "done"
    );
}

// === foreach with break ===

#[test]
fn test_e2e_foreach_break() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3, 4, 5]; foreach ($a as $v) { if ($v == 3) { break; } echo $v; }"),
        "12"
    );
}

// === foreach with continue ===

#[test]
fn test_e2e_foreach_continue() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3, 4, 5]; foreach ($a as $v) { if ($v == 3) { continue; } echo $v; }"),
        "1245"
    );
}

// === foreach preserves insertion order ===

#[test]
fn test_e2e_foreach_order_preserved() {
    assert_eq!(
        run_php("<?php $a = ['z' => 1, 'a' => 2, 'm' => 3]; $r = ''; foreach ($a as $k => $v) { $r .= $k; } echo $r;"),
        "zam"
    );
}

// === foreach with mixed keys ===

#[test]
fn test_e2e_foreach_mixed_keys() {
    assert_eq!(
        run_php("<?php $a = [0 => 'x', 'name' => 'y', 1 => 'z']; foreach ($a as $k => $v) { echo $k . $v; }"),
        "0xnamey1z"
    );
}

// === foreach accumulator ===

#[test]
fn test_e2e_foreach_sum() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3, 4, 5]; $sum = 0; foreach ($a as $v) { $sum += $v; } echo $sum;"),
        "15"
    );
}

// === foreach with array() syntax ===

#[test]
fn test_e2e_foreach_array_syntax() {
    assert_eq!(
        run_php("<?php $a = array('x', 'y', 'z'); foreach ($a as $v) { echo $v; }"),
        "xyz"
    );
}

// === Nested foreach ===

#[test]
fn test_e2e_foreach_nested() {
    assert_eq!(
        run_php("<?php $a = [[1, 2], [3, 4]]; foreach ($a as $row) { foreach ($row as $v) { echo $v; } }"),
        "1234"
    );
}

// === foreach with function result ===

#[test]
fn test_e2e_foreach_function_result() {
    assert_eq!(
        run_php("<?php function nums() { return [10, 20, 30]; } foreach (nums() as $v) { echo $v; }"),
        "102030"
    );
}

// === foreach doesn't modify original (copy semantics) ===

#[test]
fn test_e2e_foreach_copy_semantics() {
    // PHP foreach iterates over a copy of the array
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; foreach ($a as $v) { $a[] = $v * 10; } echo $a[0]; echo $a[3]; echo $a[4]; echo $a[5];"),
        "1102030"
    );
}

// === foreach with sparse array ===

#[test]
fn test_e2e_foreach_sparse() {
    assert_eq!(
        run_php("<?php $a = []; $a[0] = 'a'; $a[5] = 'b'; $a[100] = 'c'; foreach ($a as $k => $v) { echo $k . ':' . $v . ' '; }"),
        "0:a 5:b 100:c "
    );
}

// === foreach single element ===

#[test]
fn test_e2e_foreach_single_element() {
    assert_eq!(
        run_php("<?php $a = [42]; foreach ($a as $v) { echo $v; }"),
        "42"
    );
}

// === foreach with break 2 from nested ===

#[test]
fn test_e2e_foreach_break_2_nested() {
    assert_eq!(
        run_php("<?php $a = [[1, 2], [3, 4], [5, 6]]; foreach ($a as $row) { foreach ($row as $v) { if ($v == 3) { break 2; } echo $v; } }"),
        "12"
    );
}

// === foreach with continue in nested ===

#[test]
fn test_e2e_foreach_continue_nested() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; $b = ['a', 'b']; foreach ($a as $n) { foreach ($b as $l) { if ($n == 2 && $l == 'a') { continue; } echo $n . $l; } }"),
        "1a1b2b3a3b"
    );
}

// === foreach building new array ===

#[test]
fn test_e2e_foreach_build_array() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; $b = []; foreach ($a as $v) { $b[] = $v * $v; } echo $b[0]; echo $b[1]; echo $b[2];"),
        "149"
    );
}

// === foreach with key-value on int-keyed array ===

#[test]
fn test_e2e_foreach_int_key_value_sum() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; $sum = 0; foreach ($a as $k => $v) { $sum += $k + $v; } echo $sum;"),
        "63"
    );
}

// === CR11 regression: foreach on non-array emits warning ===

#[test]
fn test_e2e_foreach_int_warns() {
    assert_eq!(
        run_php("<?php foreach (42 as $v) { echo $v; } echo 'after';"),
        "\nWarning: foreach() argument must be of type array|object, int given\nafter"
    );
}

#[test]
fn test_e2e_foreach_null_warns() {
    assert_eq!(
        run_php("<?php foreach (null as $v) { echo $v; } echo 'after';"),
        "\nWarning: foreach() argument must be of type array|object, null given\nafter"
    );
}

#[test]
fn test_e2e_foreach_string_warns() {
    assert_eq!(
        run_php("<?php foreach ('abc' as $v) { echo $v; } echo 'after';"),
        "\nWarning: foreach() argument must be of type array|object, string given\nafter"
    );
}

#[test]
fn test_e2e_foreach_bool_warns() {
    assert_eq!(
        run_php("<?php foreach (true as $v) { echo $v; } echo 'after';"),
        "\nWarning: foreach() argument must be of type array|object, bool given\nafter"
    );
}

#[test]
fn test_e2e_quick_foreach_declared_object_property_accumulation() {
    assert_eq!(
        run_php("<?php
class QuickForeachRow {
    public $value;
    public $name;
    public function __construct($value, $name) {
        $this->value = $value;
        $this->name = $name;
    }
}
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[] = new QuickForeachRow(($i % 4) + 1, 'alpha');
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"),
        "480|4|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_object_property_accumulation_on_hash_array() {
    assert_eq!(
        run_php("<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[$i * 3] = json_decode('{\"value\":11,\"name\":\"alpha\"}');
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_property_pair_handles_mixed_insertion_order() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    if (($i % 2) == 0) {
        $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\"}');
    } else {
        $rows[] = json_decode('{\"name\":\"alpha\",\"value\":11}');
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_property_pair_handles_linear_storage() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\",\"x\":1,\"y\":2}');
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_property_pair_handles_hash_storage() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\",\"x\":1,\"y\":2,\"a\":3,\"b\":4,\"c\":5,\"d\":6,\"e\":7}');
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1024|11|alpha"
    );
}

#[test]
fn test_e2e_dynamic_property_cache_survives_linear_to_hash_promotion() {
    assert_eq!(
        run_php(
            "<?php
$row = json_decode('{\"value\":11,\"name\":\"alpha\",\"x\":1,\"y\":2}');
$sum = 0;
for ($i = 0; $i < 80; $i++) {
    $sum += $row->value + strlen($row->name);
    if ($i == 40) {
        $row->a = 5;
        $row->b = 6;
        $row->c = 7;
        $row->d = 8;
        $row->e = 9;
        $row->value = 13;
    }
}
echo $sum . '|' . $row->value . '|' . $row->name . '|' . $row->e;
"
        ),
        "1358|13|alpha|9"
    );
}

#[test]
fn test_e2e_quick_foreach_dynamic_second_projection_side_exit_is_exact() {
    assert_eq!(
        run_php(
            "<?php
$rows = [];
for ($i = 0; $i < 64; $i++) {
    if ($i == 40) {
        $rows[] = json_decode('{\"value\":11,\"name\":5}');
    } else {
        $rows[] = json_decode('{\"value\":11,\"name\":\"alpha\"}');
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value . '|' . $row->name;
"
        ),
        "1020|11|alpha"
    );
}

#[test]
fn test_e2e_quick_foreach_object_class_guard_side_exit() {
    assert_eq!(
        run_php("<?php
class QuickForeachLeft { public $value = 2; public $name = 'x'; }
class QuickForeachRight { public $value = 4; public $name = 'x'; }
$rows = [];
for ($i = 0; $i < 70; $i++) {
    if (($i % 2) == 0) {
        $rows[] = new QuickForeachLeft();
    } else {
        $rows[] = new QuickForeachRight();
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + strlen($row->name);
}
echo $sum . '|' . $row->value;
"),
        "280|4"
    );
}

#[test]
fn test_e2e_quick_foreach_single_long_property_projection() {
    assert_eq!(
        run_php("<?php
class QuickForeachLongRow {
    public $value;
    public function __construct($value) { $this->value = $value; }
}
$rows = [];
for ($i = 0; $i < 64; $i++) {
    $rows[] = new QuickForeachLongRow(($i % 4) + 1);
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value;
}
echo $sum . '|' . $row->value;
"),
        "160|4"
    );
}

#[test]
fn test_e2e_quick_foreach_property_type_side_exit() {
    assert_eq!(
        run_php("<?php
class QuickForeachMixedValueRow {
    public $value;
    public function __construct($value) { $this->value = $value; }
}
$rows = [];
for ($i = 0; $i < 64; $i++) {
    if ($i == 40) {
        $rows[] = new QuickForeachMixedValueRow(1.5);
    } else {
        $rows[] = new QuickForeachMixedValueRow(1);
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value;
}
echo $sum;
"),
        "64.5"
    );
}
