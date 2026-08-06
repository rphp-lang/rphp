/// E2E tests: stdlib functions — count, strlen, array_*, string functions, math, type checks.
mod common;
use common::run_php;

// === count() ===

#[test]
fn test_e2e_count_array() {
    assert_eq!(run_php("<?php echo count([1, 2, 3]);"), "3");
}

#[test]
fn test_e2e_count_empty() {
    assert_eq!(run_php("<?php echo count([]);"), "0");
}

#[test]
fn test_e2e_count_assoc() {
    assert_eq!(run_php("<?php echo count(['a' => 1, 'b' => 2]);"), "2");
}

#[test]
fn test_e2e_count_null() {
    assert_eq!(run_php("<?php echo count(null);"), "0");
}

#[test]
fn test_e2e_count_scalar() {
    assert_eq!(run_php("<?php echo count(42);"), "1");
}

// === strlen() ===

#[test]
fn test_e2e_strlen_basic() {
    assert_eq!(run_php("<?php echo strlen('hello');"), "5");
}

#[test]
fn test_e2e_strlen_empty() {
    assert_eq!(run_php("<?php echo strlen('');"), "0");
}

#[test]
fn test_e2e_strlen_number() {
    assert_eq!(run_php("<?php echo strlen(12345);"), "5");
}

// === substr() ===

#[test]
fn test_e2e_substr_basic() {
    assert_eq!(run_php("<?php echo substr('hello world', 6);"), "world");
}

#[test]
fn test_e2e_substr_with_length() {
    assert_eq!(run_php("<?php echo substr('hello world', 0, 5);"), "hello");
}

#[test]
fn test_e2e_substr_negative_start() {
    assert_eq!(run_php("<?php echo substr('hello', -3);"), "llo");
}

// === strpos() ===

#[test]
fn test_e2e_strpos_found() {
    assert_eq!(run_php("<?php echo strpos('hello world', 'world');"), "6");
}

#[test]
fn test_e2e_strpos_not_found() {
    // Returns false, which echoes as empty string
    assert_eq!(
        run_php("<?php $r = strpos('hello', 'xyz'); if (!$r) { echo 'not found'; }"),
        "not found"
    );
}

// === str_replace() ===

#[test]
fn test_e2e_str_replace() {
    assert_eq!(
        run_php("<?php echo str_replace('world', 'PHP', 'hello world');"),
        "hello PHP"
    );
}

#[test]
fn test_e2e_str_replace_multiple() {
    assert_eq!(
        run_php("<?php echo str_replace('o', '0', 'foo bar boo');"),
        "f00 bar b00"
    );
}

// === strtolower / strtoupper ===

#[test]
fn test_e2e_strtolower() {
    assert_eq!(
        run_php("<?php echo strtolower('HELLO World');"),
        "hello world"
    );
}

#[test]
fn test_e2e_strtoupper() {
    assert_eq!(
        run_php("<?php echo strtoupper('hello World');"),
        "HELLO WORLD"
    );
}

// === trim() ===

#[test]
fn test_e2e_trim() {
    assert_eq!(run_php("<?php echo trim('  hello  ');"), "hello");
}

// === explode / implode ===

#[test]
fn test_e2e_explode() {
    assert_eq!(
        run_php(
            "<?php $parts = explode(',', 'a,b,c'); echo $parts[0]; echo $parts[1]; echo $parts[2];"
        ),
        "abc"
    );
}

#[test]
fn test_e2e_implode() {
    assert_eq!(run_php("<?php echo implode('-', [1, 2, 3]);"), "1-2-3");
}

#[test]
fn test_e2e_implode_mixed_scalar_values() {
    assert_eq!(
        run_php("<?php echo implode('|', [null, false, true, 42, 1.5, 'x']);"),
        "||1|42|1.5|x"
    );
}

#[test]
fn test_e2e_chain_scalar_stdlib() {
    // Chain two scalar stdlib calls
    assert_eq!(run_php("<?php $x = strlen('hello'); echo $x;"), "5");
    assert_eq!(
        run_php("<?php $x = strlen('hello'); $y = strlen('world'); echo $x; echo $y;"),
        "55"
    );
}

#[test]
fn test_e2e_array_to_count_inline() {
    // Pass array literal directly to count — works
    assert_eq!(run_php("<?php $a = [1, 2, 3]; echo count($a);"), "3");
}

#[test]
fn test_e2e_explode_then_count() {
    assert_eq!(
        run_php("<?php $parts = explode(' ', 'a b c'); echo count($parts);"),
        "3"
    );
}

#[test]
fn test_e2e_explode_implode_round_trip() {
    assert_eq!(
        run_php("<?php $parts = explode(' ', 'a b c'); echo implode('_', $parts);"),
        "a_b_c"
    );
}

// === str_repeat ===

#[test]
fn test_e2e_str_repeat() {
    assert_eq!(run_php("<?php echo str_repeat('ab', 3);"), "ababab");
}

// === substr_count ===

#[test]
fn test_e2e_substr_count() {
    assert_eq!(
        run_php("<?php echo substr_count('hello world hello', 'hello');"),
        "2"
    );
}

// === str_contains / str_starts_with / str_ends_with (PHP 8) ===

#[test]
fn test_e2e_str_contains_true() {
    assert_eq!(
        run_php("<?php echo str_contains('hello world', 'world') ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_str_contains_false() {
    assert_eq!(
        run_php("<?php echo str_contains('hello world', 'xyz') ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_e2e_str_starts_with() {
    assert_eq!(
        run_php("<?php echo str_starts_with('hello world', 'hello') ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_str_ends_with() {
    assert_eq!(
        run_php("<?php echo str_ends_with('hello world', 'world') ? 'yes' : 'no';"),
        "yes"
    );
}

// === array_reverse ===
// NOTE: array_push/array_pop/array_shift require pass-by-reference semantics
// which we don't support yet. Those are registered but will be testable once
// we implement &$param support.

// === array_key_exists / in_array ===

#[test]
fn test_e2e_array_key_exists_true() {
    assert_eq!(
        run_php(
            "<?php $a = ['name' => 'Alice']; echo array_key_exists('name', $a) ? 'yes' : 'no';"
        ),
        "yes"
    );
}

#[test]
fn test_e2e_array_key_exists_false() {
    assert_eq!(
        run_php("<?php $a = ['name' => 'Alice']; echo array_key_exists('age', $a) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_e2e_in_array_found() {
    assert_eq!(
        run_php("<?php echo in_array(2, [1, 2, 3]) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_in_array_not_found() {
    assert_eq!(
        run_php("<?php echo in_array(5, [1, 2, 3]) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_e2e_in_array_string() {
    assert_eq!(
        run_php("<?php echo in_array('b', ['a', 'b', 'c']) ? 'yes' : 'no';"),
        "yes"
    );
}

// === array_reverse ===

#[test]
fn test_e2e_array_reverse() {
    assert_eq!(
        run_php("<?php $a = array_reverse([1, 2, 3]); echo $a[0]; echo $a[1]; echo $a[2];"),
        "321"
    );
}

// === array_merge ===

#[test]
fn test_e2e_array_merge() {
    assert_eq!(
        run_php(
            "<?php $a = array_merge([1, 2], [3, 4]); echo $a[0]; echo $a[1]; echo $a[2]; echo $a[3];"
        ),
        "1234"
    );
}

// === Type functions ===

#[test]
fn test_e2e_intval() {
    assert_eq!(run_php("<?php echo intval('42abc');"), "42");
}

#[test]
fn test_e2e_intval_float() {
    assert_eq!(run_php("<?php echo intval(3);"), "3");
}

#[test]
fn test_e2e_strval() {
    assert_eq!(run_php("<?php echo strval(42);"), "42");
}

#[test]
fn test_e2e_is_array_true() {
    assert_eq!(
        run_php("<?php echo is_array([1, 2]) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_is_array_false() {
    assert_eq!(run_php("<?php echo is_array(42) ? 'yes' : 'no';"), "no");
}

#[test]
fn test_e2e_is_string() {
    assert_eq!(
        run_php("<?php echo is_string('hello') ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_is_int() {
    assert_eq!(run_php("<?php echo is_int(42) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_e2e_is_null_check() {
    assert_eq!(run_php("<?php echo is_null(null) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_e2e_is_numeric_int() {
    assert_eq!(run_php("<?php echo is_numeric(42) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_e2e_is_numeric_string() {
    assert_eq!(
        run_php("<?php echo is_numeric('3.14') ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_is_numeric_false() {
    assert_eq!(
        run_php("<?php echo is_numeric('hello') ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_e2e_gettype() {
    assert_eq!(run_php("<?php echo gettype(42);"), "integer");
    assert_eq!(run_php("<?php echo gettype('hi');"), "string");
    assert_eq!(run_php("<?php echo gettype(null);"), "NULL");
    assert_eq!(run_php("<?php echo gettype(true);"), "boolean");
    assert_eq!(run_php("<?php echo gettype([1]);"), "array");
}

// === Math functions ===

#[test]
fn test_e2e_abs() {
    assert_eq!(run_php("<?php echo abs(-5);"), "5");
    assert_eq!(run_php("<?php echo abs(3);"), "3");
}

#[test]
fn test_e2e_max_min() {
    assert_eq!(run_php("<?php echo max(3, 7);"), "7");
    assert_eq!(run_php("<?php echo min(3, 7);"), "3");
}

#[test]
fn test_e2e_pow() {
    assert_eq!(run_php("<?php echo pow(2, 10);"), "1024");
}

// === var_dump ===

#[test]
fn test_e2e_var_dump_int() {
    assert_eq!(run_php("<?php var_dump(42);"), "int(42)\n");
}

#[test]
fn test_e2e_var_dump_string() {
    assert_eq!(run_php("<?php var_dump('hello');"), "string(5) \"hello\"\n");
}

#[test]
fn test_e2e_var_dump_array() {
    assert_eq!(
        run_php("<?php var_dump([1, 2]);"),
        "array(2) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(2)\n}\n"
    );
}

#[test]
fn test_e2e_var_dump_null() {
    assert_eq!(run_php("<?php var_dump(null);"), "NULL\n");
}

#[test]
fn test_e2e_var_dump_bool() {
    assert_eq!(run_php("<?php var_dump(true);"), "bool(true)\n");
    assert_eq!(run_php("<?php var_dump(false);"), "bool(false)\n");
}

// === print_r ===

#[test]
fn test_e2e_print_r_array() {
    assert_eq!(
        run_php("<?php print_r([1, 2, 3]);"),
        "Array\n(\n    [0] => 1\n    [1] => 2\n    [2] => 3\n)\n"
    );
}

// === Practical combinations ===

#[test]
fn test_e2e_foreach_count_loop() {
    assert_eq!(
        run_php(
            "<?php $a = [10, 20, 30]; $n = count($a); for ($i = 0; $i < $n; $i++) { echo $a[$i]; }"
        ),
        "102030"
    );
}

#[test]
fn test_e2e_explode_foreach() {
    assert_eq!(
        run_php("<?php $csv = 'a,b,c'; foreach (explode(',', $csv) as $v) { echo $v; }"),
        "abc"
    );
}

#[test]
fn test_e2e_string_processing_pipeline() {
    assert_eq!(
        run_php(
            "<?php $s = '  Hello World  '; $s = trim($s); $s = strtolower($s); $s = str_replace(' ', '_', $s); echo $s;"
        ),
        "hello_world"
    );
}

#[test]
fn test_e2e_array_filter_manual() {
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3, 4, 5, 6]; $even = []; foreach ($a as $v) { if ($v % 2 == 0) { $even[] = $v; } } echo implode(',', $even);"
        ),
        "2,4,6"
    );
}

#[test]
fn test_e2e_word_count() {
    assert_eq!(
        run_php(
            "<?php $s = 'the quick brown fox jumps over the lazy dog'; $words = explode(' ', $s); echo count($words);"
        ),
        "9"
    );
}
