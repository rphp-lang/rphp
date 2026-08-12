// === Array literal & echo ===

#[test]
fn test_e2e_array_echo_prints_array() {
    // echo $arr prints "Array" (PHP behavior)
    assert_eq!(run_php("<?php $a = [1, 2, 3]; echo $a;"), "Array");
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
fn test_e2e_array_missing_key_returns_null() {
    // Accessing non-existent key returns null (echoes as empty string)
    assert_eq!(run_php("<?php $a = [1, 2]; echo $a[5]; echo 'end';"), "end");
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
    // Loop reads past the end of the array — should get nulls (empty echo)
    assert_eq!(
        run_php(
            "<?php $a = [10, 20]; $r = ''; for ($i = 0; $i < 5; $i++) { $v = $a[$i]; if ($v) { $r .= $v; } else { $r .= '_'; } } echo $r;"
        ),
        "1020___"
    );
}

#[test]
fn test_e2e_array_negative_index() {
    // PHP arrays don't have negative indexing (unlike Python) — returns null
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; if ($a[-1]) { echo 'yes'; } else { echo 'no'; }"),
        "no"
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
        "abc"
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
        "str"
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
