// === Array literal & echo ===

#[test]
fn natural_sort_constants_order_numeric_key_fragments() {
    assert_eq!(
        run_php(
            "<?php $values = ['item10' => 10, 'item2' => 2, 'Item1' => 1]; ksort($values, SORT_NATURAL | SORT_FLAG_CASE); echo implode(',', array_keys($values)), ':', SORT_LOCALE_STRING;"
        ),
        "Item1,item2,item10:5"
    );
}

#[test]
fn test_e2e_array_echo_prints_array() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; echo $a;"),
        "\nWarning: Array to string conversion in <main> on line 1\nArray"
    );
}

// === Array access ===

#[test]
fn test_e2e_array_int_access() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; echo $a[0]; echo $a[1]; echo $a[2];"),
        "102030"
    );
}

#[test]
fn test_e2e_array_string_key_access() {
    assert_eq!(
        run_php("<?php $a = ['name' => 'Alice', 'age' => 30]; echo $a['name']; echo $a['age'];"),
        "Alice30"
    );
}

#[test]
fn test_e2e_string_key_position_cache_validates_reordered_arrays() {
    assert_eq!(
        run_php(
            "<?php
for ($i = 0; $i < 4; $i++) {
    if (($i % 2) == 0) {
        $row = ['a' => 1, 'b' => 2];
    } else {
        $row = ['b' => 20, 'a' => 10];
    }
    echo $row['a'];
}
"
        ),
        "110110"
    );
}

#[test]
fn test_e2e_array_mixed_keys() {
    assert_eq!(
        run_php(
            "<?php $a = [0 => 'a', 'x' => 'b', 1 => 'c']; echo $a[0]; echo $a['x']; echo $a[1];"
        ),
        "abc"
    );
}

#[test]
fn key_reports_the_initial_array_cursor_key() {
    assert_eq!(
        run_php("<?php $named = ['first' => 1, 'second' => 2]; echo key($named), '|'; $indexed = [7 => 'x']; echo key($indexed), '|'; var_dump(key([]));"),
        "first|7|NULL\n"
    );
}

#[test]
fn test_e2e_array_unpack_reindexes_integer_keys_and_preserves_string_keys() {
    assert_eq!(
        run_php(
            "<?php class Providers { public const IPS = ['primary' => '10.0.0.1', 20 => '10.0.0.2']; } $values = [...Providers::IPS, '127.0.0.1', 'primary' => 'override']; echo $values['primary'], '|', $values[0], '|', $values[1];"
        ),
        "override|10.0.0.2|127.0.0.1"
    );
}

#[test]
fn test_e2e_array_missing_key_returns_null() {
    // Accessing a non-existent key warns and produces null (which echoes empty).
    assert_eq!(
        run_php("<?php $a = [1, 2]; echo $a[5]; echo 'end';"),
        "\nWarning: Undefined array key 5 in <main> on line 1\nend"
    );
}

// === Array assignment ===

#[test]
fn test_e2e_array_assign_element() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; $a[1] = 42; echo $a[0]; echo $a[1]; echo $a[2];"),
        "1423"
    );
}

#[test]
fn test_e2e_array_assign_new_key() {
    assert_eq!(run_php("<?php $a = [1, 2]; $a[5] = 99; echo $a[5];"), "99");
}

#[test]
fn test_e2e_array_assign_string_key() {
    assert_eq!(
        run_php("<?php $a = ['x' => 1]; $a['y'] = 2; echo $a['x']; echo $a['y'];"),
        "12"
    );
}

// === Array push ($a[] = val) ===

#[test]
fn test_e2e_array_push() {
    assert_eq!(
        run_php("<?php $a = []; $a[] = 'first'; $a[] = 'second'; echo $a[0]; echo $a[1];"),
        "firstsecond"
    );
}

#[test]
fn test_e2e_array_push_auto_creates() {
    // $a[] = val on undefined var auto-creates array
    assert_eq!(
        run_php("<?php $a[] = 10; $a[] = 20; echo $a[0]; echo $a[1];"),
        "1020"
    );
}

#[test]
fn test_e2e_array_push_continues_index() {
    assert_eq!(run_php("<?php $a = [10, 20]; $a[] = 30; echo $a[2];"), "30");
}

#[test]
fn array_access_objects_dispatch_dimension_operations() {
    assert_eq!(
        run_php(
            "<?php
class Bag implements ArrayAccess {
    private array $values = [];
    public function offsetSet($offset, $value): void { $this->values[$offset] = $value; }
    public function offsetGet($offset): mixed { return $this->values[$offset] ?? null; }
    public function offsetExists($offset): bool { return isset($this->values[$offset]); }
    public function offsetUnset($offset): void { unset($this->values[$offset]); }
}
$bag = new Bag();
$bag['name'] = 'value';
echo $bag['name'], '|';
unset($bag['name']);
echo $bag['name'] ?? 'missing';
"
        ),
        "value|missing"
    );
}

#[test]
fn spl_object_storage_uses_object_identity_and_iterates_objects() {
    assert_eq!(
        run_php(
            "<?php
$first = new stdClass();
$second = new stdClass();
$storage = new SplObjectStorage();
$storage[$first] = 'first';
$storage->attach($second, 'second');
echo $storage[$first], ':', $storage[$second], ':', $storage->count(), ':';
echo isset($storage[$first]) ? 'yes' : 'no';
echo isset($storage[new stdClass()]) ? 'bad' : 'ok', '|';
foreach ($storage as $index => $object) {
    echo $index, '=', $object === $first ? 'first' : 'second', ';';
}
$storage->detach($first);
echo '|', $storage->contains($first) ? 'bad' : 'ok', ':', $storage->count(), '|';
class StorageHolder {
    public SplObjectStorage $storage;
    public function __construct() { $this->storage = new SplObjectStorage(); }
    public function add(object $object): void { $this->storage[$object] = 'nested'; }
}
$holder = new StorageHolder();
$holder->add($first);
echo $holder->storage[$first];
"
        ),
        "first:second:2:yesok|0=first;1=second;|ok:1|nested"
    );
}

// === Array in loops ===

#[test]
fn test_e2e_array_build_in_loop() {
    assert_eq!(
        run_php(
            "<?php $a = []; for ($i = 0; $i < 5; $i++) { $a[] = $i * 2; } echo $a[0]; echo $a[2]; echo $a[4];"
        ),
        "048"
    );
}

#[test]
fn test_e2e_array_iterate_by_index() {
    assert_eq!(
        run_php(
            "<?php $a = [10, 20, 30, 40]; $sum = 0; for ($i = 0; $i < 4; $i++) { $sum += $a[$i]; } echo $sum;"
        ),
        "100"
    );
}

// === Array with functions ===

#[test]
fn test_e2e_array_pass_to_function() {
    assert_eq!(
        run_php("<?php function first($arr) { return $arr[0]; } $a = [42, 99]; echo first($a);"),
        "42"
    );
}

#[test]
fn test_e2e_array_return_from_function() {
    assert_eq!(
        run_php(
            "<?php function make() { $a = []; $a[] = 1; $a[] = 2; return $a; } $r = make(); echo $r[0]; echo $r[1];"
        ),
        "12"
    );
}

// === array() syntax ===

#[test]
fn test_e2e_array_long_syntax() {
    assert_eq!(
        run_php("<?php $a = array(1, 2, 3); echo $a[0]; echo $a[1]; echo $a[2];"),
        "123"
    );
}

#[test]
fn test_e2e_array_long_syntax_with_keys() {
    assert_eq!(
        run_php("<?php $a = array('a' => 10, 'b' => 20); echo $a['a']; echo $a['b'];"),
        "1020"
    );
}

// === Nested arrays ===

#[test]
fn test_e2e_nested_array_access() {
    assert_eq!(
        run_php(
            "<?php $a = [[1, 2], [3, 4]]; echo $a[0][0]; echo $a[0][1]; echo $a[1][0]; echo $a[1][1];"
        ),
        "1234"
    );
}

// === Array truthiness ===

#[test]
fn test_e2e_empty_array_is_falsy() {
    assert_eq!(
        run_php("<?php $a = []; if ($a) { echo 'yes'; } else { echo 'no'; }"),
        "no"
    );
}

#[test]
fn test_e2e_nonempty_array_is_truthy() {
    assert_eq!(
        run_php("<?php $a = [1]; if ($a) { echo 'yes'; } else { echo 'no'; }"),
        "yes"
    );
}

// === Array auto-create on assign ===

#[test]
fn test_e2e_array_auto_create_on_dim_assign() {
    assert_eq!(run_php("<?php $a[0] = 'hello'; echo $a[0];"), "hello");
}

#[test]
fn test_e2e_array_auto_create_string_key() {
    assert_eq!(run_php("<?php $a['key'] = 'val'; echo $a['key'];"), "val");
}

// === String offset access ===

#[test]
fn test_e2e_string_offset_access() {
    assert_eq!(run_php("<?php $s = 'hello'; echo $s[0]; echo $s[4];"), "ho");
}

// === Trailing comma ===

#[test]
fn test_e2e_array_trailing_comma() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3,]; echo $a[0]; echo $a[2];"),
        "13"
    );
}

// === count() internal function ===

#[test]
fn test_e2e_array_in_switch() {
    assert_eq!(
        run_php(
            "<?php $cmds = ['run', 'stop', 'wait']; switch ($cmds[1]) { case 'run': echo 'R'; break; case 'stop': echo 'S'; break; default: echo 'X'; }"
        ),
        "S"
    );
}

// === Edge cases: out of bounds, empty iteration ===

#[test]
fn test_e2e_array_loop_past_end() {
    // Loop reads past the end of the array — each missing read warns and yields null.
    assert_eq!(
        run_php(
            "<?php $a = [10, 20]; $r = ''; for ($i = 0; $i < 5; $i++) { $v = $a[$i]; if ($v) { $r .= $v; } else { $r .= '_'; } } echo $r;"
        ),
        "\nWarning: Undefined array key 2 in <main> on line 1\n\nWarning: Undefined array key 3 in <main> on line 1\n\nWarning: Undefined array key 4 in <main> on line 1\n1020___"
    );
}

#[test]
fn test_e2e_array_negative_index() {
    // PHP arrays don't have Python-style negative indexing.
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; if ($a[-1]) { echo 'yes'; } else { echo 'no'; }"),
        "\nWarning: Undefined array key -1 in <main> on line 1\nno"
    );
}

#[test]
fn test_e2e_array_overwrite_with_different_type() {
    assert_eq!(
        run_php("<?php $a = [1, 'hello', 3]; $a[1] = 42; echo $a[0]; echo $a[1]; echo $a[2];"),
        "1423"
    );
}

#[test]
fn test_e2e_array_sparse_keys() {
    // Non-contiguous integer keys
    assert_eq!(
        run_php(
            "<?php $a = []; $a[0] = 'a'; $a[5] = 'b'; $a[100] = 'c'; echo $a[0]; echo $a[5]; echo $a[100]; echo $a[1];"
        ),
        "abc\nWarning: Undefined array key 1 in <main> on line 1\n"
    );
}

#[test]
fn test_e2e_array_bool_key_coercion() {
    // PHP coerces true→1, false→0 as array keys
    assert_eq!(
        run_php("<?php $a = []; $a[true] = 'T'; $a[false] = 'F'; echo $a[1]; echo $a[0];"),
        "TF"
    );
}

#[test]
fn test_e2e_array_null_key_coercion() {
    // PHP coerces null→"" as array key
    assert_eq!(run_php("<?php $a = []; $a[null] = 'N'; echo $a[''];"), "N");
}

#[test]
fn illegal_array_offsets_throw_catchable_contextual_type_errors() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    fn() => [new stdClass() => 1],
    function() { $array = []; $array[[]] = 1; },
    function() { $array = []; return $array[[]]; },
] as $operation) {
    try { $operation(); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
$array = [];
try { isset($array[[]]); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { empty($array[[]]); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { unset($array[[]]); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { $holder = (object)['values' => []]; $holder->values[[]] = 1; } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { $reference =& $array[[]]; } catch (TypeError $error) { echo $error->getMessage(); }
"#,
        ),
        concat!(
            "Illegal offset type\n",
            "Illegal offset type\n",
            "Illegal offset type\n",
            "Illegal offset type in isset or empty\n",
            "Illegal offset type in isset or empty\n",
            "Illegal offset type in unset\n",
            "Illegal offset type\n",
            "Illegal offset type",
        )
    );
}

#[test]
fn test_e2e_array_empty_literal() {
    assert_eq!(
        run_php("<?php $a = []; if ($a) { echo 'yes'; } else { echo 'empty'; }"),
        "empty"
    );
}

#[test]
fn test_e2e_array_push_after_explicit_high_key() {
    // $a[] should continue from max int key + 1
    assert_eq!(
        run_php("<?php $a = [10 => 'a']; $a[] = 'b'; echo $a[10]; echo $a[11];"),
        "ab"
    );
}

// === CR10 regression: numeric-string key normalization ===

#[test]
fn test_e2e_array_numeric_string_key_normalized() {
    // "1" and 1 are the same key in PHP
    assert_eq!(run_php("<?php $a = []; $a['1'] = 'x'; echo $a[1];"), "x");
}

#[test]
fn test_e2e_array_numeric_string_key_push_continues() {
    // After $a["5"] = ..., $a[] should produce key 6
    assert_eq!(
        run_php("<?php $a = []; $a['5'] = 'x'; $a[] = 'y'; echo $a[6];"),
        "y"
    );
}

#[test]
fn test_e2e_array_non_numeric_string_key_preserved() {
    // "01" is NOT normalized to 1 (leading zero)
    assert_eq!(
        run_php("<?php $a = []; $a['01'] = 'x'; if ($a[1]) { echo 'int'; } else { echo 'str'; }"),
        "\nWarning: Undefined array key 1 in <main> on line 1\nstr"
    );
}

#[test]
fn test_e2e_array_noncanonical_numeric_strings_stay_strings() {
    assert_eq!(
        run_php(
            "<?php $a = ['-0' => 'a', '+1' => 'b', ' 1' => 'c']; echo $a['-0']; echo $a['+1']; echo $a[' 1'];"
        ),
        "abc"
    );
}

// === CR10 regression: string offset is byte-based ===

#[test]
fn test_e2e_string_offset_byte_based() {
    // PHP strings are byte-based — multi-byte char spans multiple offsets
    // "č" is 2 bytes (0xC4 0x8D), so $s[0] and $s[1] are each single bytes
    assert_eq!(run_php("<?php $s = 'ax'; echo $s[0]; echo $s[1];"), "ax");
}

#[test]
fn test_e2e_string_negative_offset() {
    // Negative offset counts from end (byte-based)
    assert_eq!(run_php("<?php $s = 'hello'; echo $s[-1];"), "o");
}

#[test]
fn string_offset_miss_warns_at_access_and_isset_stays_silent() {
    assert_eq!(
        run_php(
            "<?php
$value = 'abc';
var_dump($value[3]);
var_dump($value[-4]);
var_dump(isset($value[3]));
var_dump(@$value[4]);
var_dump($value[-1]);
set_error_handler(function($code, $message) { echo 'handled:', $code, ':', $message, PHP_EOL; return true; });
var_dump($value[8]);
$empty = '';
var_dump(isset($empty[0][0]));
"
        ),
        "\nWarning: Uninitialized string offset 3 in <main> on line 3\nstring(0) \"\"\n\nWarning: Uninitialized string offset -4 in <main> on line 4\nstring(0) \"\"\nbool(false)\nstring(0) \"\"\nstring(1) \"c\"\nhandled:2:Uninitialized string offset 8\nstring(0) \"\"\nbool(false)\n"
    );
}

#[test]
fn scalar_offset_reads_warn_while_isset_and_suppression_stay_silent() {
    assert_eq!(
        run_php(
            "<?php
$integer = 1;
var_dump($integer[0]);
$boolean = true;
var_dump($boolean['key']);
$null = null;
var_dump($null[0]);
var_dump(isset($integer[0]));
var_dump(@$boolean[0]);
set_error_handler(function($code, $message) { echo 'handled:', $code, ':', $message, PHP_EOL; return true; });
var_dump($null[1]);
"
        ),
        "\nWarning: Trying to access array offset on value of type int in <main> on line 3\nNULL\n\nWarning: Trying to access array offset on value of type bool in <main> on line 5\nNULL\n\nWarning: Trying to access array offset on value of type null in <main> on line 7\nNULL\nbool(false)\nNULL\nhandled:2:Trying to access array offset on value of type null\nNULL\n"
    );
}

#[test]
fn suppressed_array_access_keeps_offset_get_diagnostics_silent() {
    assert_eq!(
        run_php(
            "<?php
class SilentOffsets implements ArrayAccess {
    public function offsetGet($offset): mixed { $value = null; return $value[0]; }
    public function offsetSet($offset, $value): void {}
    public function offsetUnset($offset): void {}
    public function offsetExists($offset): bool { return true; }
}
var_dump(@(new SilentOffsets)[0]);
"
        ),
        "NULL\n"
    );
}

#[test]
fn missing_array_keys_warn_while_silent_fetches_remain_silent() {
    assert_eq!(
        run_php(
            "<?php
$values = [];
var_dump($values[0]);
var_dump($values[-1]);
var_dump($values['name']);
var_dump($values['1']);
var_dump(isset($values['missing']));
var_dump(empty($values['missing']));
var_dump(@$values['suppressed']);
set_error_handler(function($code, $message) { echo 'handled:', $code, ':', $message, PHP_EOL; return true; });
var_dump($values['handled']);
"
        ),
        "\nWarning: Undefined array key 0 in <main> on line 3\nNULL\n\nWarning: Undefined array key -1 in <main> on line 4\nNULL\n\nWarning: Undefined array key \"name\" in <main> on line 5\nNULL\n\nWarning: Undefined array key 1 in <main> on line 6\nNULL\nbool(false)\nbool(true)\nNULL\nhandled:2:Undefined array key \"handled\"\nNULL\n"
    );
}

// === Inspired by PHP test suite ===

#[test]
fn test_e2e_array_sum_loop() {
    // Build array, sum its elements
    assert_eq!(
        run_php(
            "<?php $a = []; for ($i = 1; $i <= 5; $i++) { $a[] = $i; } $sum = 0; for ($i = 0; $i < 5; $i++) { $sum += $a[$i]; } echo $sum;"
        ),
        "15"
    );
}

#[test]
fn test_e2e_array_overwrite() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; $a[0] = $a[0] + $a[1] + $a[2]; echo $a[0];"),
        "6"
    );
}

#[test]
fn test_e2e_array_as_accumulator() {
    // Use array to count occurrences
    assert_eq!(
        run_php(
            "<?php $counts = [0, 0, 0]; for ($i = 0; $i < 9; $i++) { $idx = $i % 3; $counts[$idx] = $counts[$idx] + 1; } echo $counts[0]; echo $counts[1]; echo $counts[2];"
        ),
        "333"
    );
}

#[test]
fn test_e2e_array_with_ternary_values() {
    assert_eq!(
        run_php(
            "<?php $a = []; for ($i = 0; $i < 4; $i++) { $a[] = ($i % 2 == 0) ? 'E' : 'O'; } echo $a[0]; echo $a[1]; echo $a[2]; echo $a[3];"
        ),
        "EOEO"
    );
}

#[test]
fn test_e2e_function_builds_array() {
    assert_eq!(
        run_php(
            "<?php function range_arr($start, $end) { $a = []; $i = $start; while ($i <= $end) { $a[] = $i; $i++; } return $a; } $r = range_arr(3, 7); echo $r[0]; echo $r[2]; echo $r[4];"
        ),
        "357"
    );
}

#[test]
fn postfix_offsets_apply_uniformly_to_literal_and_match_atoms() {
    assert_eq!(
        run_php(
            r#"<?php
echo [10, 20][1] . ':';
echo 'abc'[1] . ':';
echo match (2) {
    1 => ['wrong'],
    2 => ['right'],
}[0];
"#,
        ),
        "20:b:right"
    );
}

#[test]
fn nested_array_dimension_append_is_a_mutation_target() {
    assert_eq!(
        run_php(
            "<?php $values = []; $key = 'group'; $values[$key][] = 'first'; $values[$key][] = 'second'; echo implode(',', $values[$key]);"
        ),
        "first,second"
    );
}

#[test]
fn destructuring_assigns_into_array_dimensions() {
    assert_eq!(
        run_php(
            "<?php $headers = []; [$headers['user'], $headers['password']] = ['alice', 'secret']; echo $headers['user'], ':', $headers['password'];"
        ),
        "alice:secret"
    );
}

#[test]
fn array_replace_overwrites_string_and_integer_keys_across_inputs() {
    assert_eq!(
        run_php(
            "<?php $value = array_replace(['name' => 'base', 0 => 'zero'], ['name' => 'middle', 0 => 'one'], ['name' => 'final']); echo $value['name'], ':', $value[0];"
        ),
        "final:one"
    );
}

#[test]
fn foreach_destructures_values_with_optional_keys() {
    assert_eq!(
        run_php(
            "<?php foreach ([[1, 2], [3, 4]] as $key => [$left, $right]) { echo $key, ':', $left + $right, '|'; }"
        ),
        "0:3|1:7|"
    );
}
#[test]
fn array_merge_accepts_zero_one_and_many_arrays() {
    assert_eq!(
        run_php(
            r#"<?php
echo count(array_merge()), '|';
$one = array_merge(['first' => 1, 8 => 'a']);
echo $one['first'], ':', $one[0], '|';
$many = array_merge(['same' => 1, 4 => 'a'], ['same' => 2, 9 => 'b'], ['tail' => 3]);
echo $many['same'], ':', $many[0], ':', $many[1], ':', $many['tail'];
"#,
        ),
        "0|1:a|2:a:b:3"
    );
}

#[test]
fn array_walk_recursive_mutates_only_leaf_values_and_accepts_userdata() {
    assert_eq!(
        run_php(
            r#"<?php
$values = ['first' => 1, 'nested' => [2, 'deep' => 3]];
array_walk_recursive($values, static function (&$value, $key, $prefix) {
    $value = $prefix . $key . ':' . $value;
}, '>');
echo $values['first'], '|', $values['nested'][0], '|', $values['nested']['deep'];

$entropy = ['name' => 'value', 'nested' => [1, true]];
array_walk_recursive($entropy, static function (&$value) { $value = null; });
echo '|', serialize($entropy);
"#,
        ),
        ">first:1|>0:2|>deep:3|a:2:{s:4:\"name\";N;s:6:\"nested\";a:2:{i:0;N;i:1;N;}}"
    );
}

#[test]
fn spl_priority_queue_orders_array_priorities_and_honors_extract_flags() {
    assert_eq!(
        run_php(
            r#"<?php
$queue = new SplPriorityQueue();
$queue->insert(['low'], [1, 9]);
$queue->insert(['high-late'], [2, 1]);
$queue->insert(['high-early'], [2, 8]);
echo $queue->count(), ':', $queue->isEmpty() ? 'empty' : 'ready', '|';
foreach ($queue as [$name]) {
    echo $name, ',';
}
$queue->setExtractFlags(SplPriorityQueue::EXTR_BOTH);
$queue->rewind();
$both = $queue->current();
echo '|', $both['data'][0], ':', $both['priority'][0], ':', $both['priority'][1];
"#,
        ),
        "3:ready|high-early,high-late,low,|high-early:2:8"
    );
}

#[test]
fn reading_a_plain_object_dimension_throws_a_catchable_error() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainDimensionObject {}
$object = new PlainDimensionObject;
try {
    echo $object['key'];
} catch (Error $error) {
    echo get_class($error), '|', $error->getMessage();
}
"#,
        ),
        "Error|Cannot use object of type PlainDimensionObject as array"
    );
}

#[test]
fn mutating_plain_object_dimensions_throws_catchable_errors() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainMutationObject {}
class ObjectHolder { public $value; }
foreach (['write', 'append', 'unset', 'reference', 'property'] as $operation) {
    $object = new PlainMutationObject;
    try {
        if ($operation === 'write') {
            $object['key'] = 1;
        } elseif ($operation === 'append') {
            $object[] = 1;
        } elseif ($operation === 'unset') {
            unset($object['key']);
        } elseif ($operation === 'reference') {
            $reference =& $object['key'];
        } else {
            $holder = new ObjectHolder;
            $holder->value = $object;
            $holder->value['key'] = 1;
        }
    } catch (Error $error) {
        echo $operation, '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "write|Cannot use object of type PlainMutationObject as array\n",
            "append|Cannot use object of type PlainMutationObject as array\n",
            "unset|Cannot use object of type PlainMutationObject as array\n",
            "reference|Cannot use object of type PlainMutationObject as array\n",
            "property|Cannot use object of type PlainMutationObject as array\n",
        )
    );
}

#[test]
fn destructuring_a_closure_reports_a_catchable_error() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    [$value] = static function () {};
} catch (Error $error) {
    echo get_class($error), '|', $error->getMessage();
}
"#,
        ),
        "Error|Cannot use object of type Closure as array"
    );
}
