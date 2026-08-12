// ── array_shift / array_pop correctness (PHP-compatible key behavior) ──

#[test]
fn test_e2e_array_shift_reindex() {
    // PHP: array_shift removes first element, renumbers integer keys from 0.
    // String keys are preserved unchanged.
    assert_eq!(
        run_php(
            "<?php
$arr = [10, 20, 30];
array_shift($arr);
$arr[] = 40;
foreach ($arr as $k => $v) { echo $k . ':' . $v . ' '; }
"
        ),
        "0:20 1:30 2:40 "
    );
}

#[test]
fn test_e2e_array_shift_mixed_keys() {
    // PHP: after shift, int keys are renumbered but string keys preserved.
    assert_eq!(
        run_php(
            "<?php
$arr = [1, 2, 3];
array_shift($arr);
$arr['x'] = 9;
$arr[] = 5;
foreach ($arr as $k => $v) { echo $k . ':' . $v . ' '; }
"
        ),
        "0:2 1:3 x:9 2:5 "
    );
}

#[test]
fn test_e2e_array_pop_then_append() {
    // PHP: pop removes last, next [] key continues from highest existing + 1.
    assert_eq!(
        run_php(
            "<?php
$arr = [1, 2, 3];
array_pop($arr);
$arr[] = 4;
$arr['x'] = 9;
$arr[] = 5;
foreach ($arr as $k => $v) { echo $k . ':' . $v . ' '; }
"
        ),
        "0:1 1:2 2:4 x:9 3:5 "
    );
}

#[test]
fn test_e2e_array_pop_then_shift_then_append() {
    // Combined pop+shift+append sequence.
    assert_eq!(
        run_php(
            "<?php
$arr = [10, 20, 30, 40];
array_pop($arr);
array_shift($arr);
$arr[] = 50;
foreach ($arr as $k => $v) { echo $k . ':' . $v . ' '; }
"
        ),
        "0:20 1:30 2:50 "
    );
}

#[test]
fn test_e2e_array_pop_sparse_hash_autoindex() {
    // P1 regression: sparse hash array — pop should only decrement next_int_key
    // if popped int key == next_int_key - 1, not recalc from max(remaining).
    // PHP: $b=[]; $b[0]=1; $b[2]=2; array_pop($b); $b[]=3; → 0:1 2:3
    assert_eq!(
        run_php(
            "<?php
$b = [];
$b[0] = 1;
$b[2] = 2;
array_pop($b);
$b[] = 3;
foreach ($b as $k => $v) { echo $k . ':' . $v . ' '; }
"
        ),
        "0:1 2:3 "
    );
}

#[test]
fn test_e2e_array_pop_nonboundary_key() {
    // Pop element whose int key is NOT next_int_key - 1 → next_int_key unchanged.
    // $a = []; $a[0]='a'; $a[5]='b'; $a[3]='c';
    // Insertion order: 0,5,3. Pop removes key 3. next_int_key was 6, stays 6.
    assert_eq!(
        run_php(
            "<?php
$a = [];
$a[0] = 'a';
$a[5] = 'b';
$a[3] = 'c';
array_pop($a);
$a[] = 'd';
foreach ($a as $k => $v) { echo $k . ':' . $v . ' '; }
"
        ),
        "0:a 5:b 6:d "
    );
}

#[test]
fn test_e2e_array_pop_string_key_last() {
    // Pop string-keyed last element — next_int_key unchanged.
    assert_eq!(
        run_php(
            "<?php
$a = [0 => 'x', 5 => 'y', 'z' => 'w'];
array_pop($a);
$a[] = 'q';
foreach ($a as $k => $v) { echo $k . ':' . $v . ' '; }
"
        ),
        "0:x 5:y 6:q "
    );
}

#[test]
fn test_e2e_array_literal_key_storage_detaches_on_source_mutation() {
    assert_eq!(
        run_php(
            "<?php
$key = 'name';
$array = [$key => 42];
$key .= '-changed';
echo $key . '|' . $array['name'];
"
        ),
        "name-changed|42"
    );
}

#[test]
fn test_hot_dynamic_hash_entry_update_preserves_results() {
    assert_eq!(
        run_php(
            "<?php
$values = ['left' => 3, 'right' => 5];
$key = 'left';
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $values[$key] + $i;
    if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; }
}
echo $values['left'] . '|' . $values['right'];
"
        ),
        "2453|2505"
    );
}

#[test]
fn test_hot_hash_update_detaches_shared_array_before_direct_replacement() {
    assert_eq!(
        run_php(
            "<?php
$values = ['left' => 3, 'right' => 5];
$original = $values;
$key = 'left';
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $values[$key] + $i;
    if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; }
}
echo $values['left'] . '|' . $values['right'] . '|';
echo $original['left'] . '|' . $original['right'];
"
        ),
        "2453|2505|3|5"
    );
}

#[test]
fn test_hot_hash_update_deoptimizes_on_long_overflow() {
    assert_eq!(
        run_php(
            "<?php
$values = ['value' => 9223372036854775772];
for ($i = 0; $i < 40; $i++) {
    $values['value'] = $values['value'] + 1;
}
echo gettype($values['value']);
"
        ),
        "double"
    );
}

#[test]
fn nested_dimension_write_evaluates_keys_once_and_rebuilds_cow_parents() {
    assert_eq!(
        run_php(
            r#"<?php
$calls = 0;
function key_once($value) { global $calls; $calls = $calls + 1; return $value; }
$original = ['outer' => ['inner' => ['keep' => 1]]];
$changed = $original;
$changed[key_once('outer')][key_once('inner')][key_once('value')] = 42;
echo $changed['outer']['inner']['value'] . ':';
echo $changed['outer']['inner']['keep'] . ':';
var_dump(isset($original['outer']['inner']['value']));
echo $calls;
"#,
        ),
        "42:1:bool(false)\n3"
    );

    assert_eq!(
        run_php(
            r#"<?php
$created['first']['second'] = 'ready';
echo $created['first']['second'];
"#,
        ),
        "ready"
    );

    assert_eq!(
        run_php(
            r#"<?php
class NestedStore {
    public $values = [];
    public static $staticValues = [];
}
$store = new NestedStore();
$store->values['outer']['value'] = 42;
NestedStore::$staticValues['outer']['value'] = 43;
echo $store->values['outer']['value'] . ':';
echo NestedStore::$staticValues['outer']['value'];
unset($store->values['outer']['value']);
unset(NestedStore::$staticValues['outer']['value']);
echo ':';
var_dump(isset($store->values['outer']['value']));
var_dump(isset(NestedStore::$staticValues['outer']['value']));
"#,
        ),
        "42:43:bool(false)\nbool(false)\n"
    );
}

#[test]
fn array_union_preserves_left_values_order_and_cow_ownership() {
    assert_eq!(
        run_php(
            r#"<?php
$left = ['shared' => 'left', 2 => 'two'];
$right = ['shared' => 'right', 0 => 'zero', 'new' => 'value'];
$union = $left + $right;
$union['shared'] = 'changed';
foreach ($union as $key => $value) { echo $key . '=' . $value . '|'; }
echo $left['shared'];
"#,
        ),
        "shared=changed|2=two|0=zero|new=value|left"
    );
}
