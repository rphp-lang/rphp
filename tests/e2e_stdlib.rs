/// E2E tests: stdlib functions — count, strlen, array_*, string functions, math, type checks.
mod common;
use common::run_php;

#[test]
fn addcslashes_preserves_php_reference_escaping_rules() {
    assert_eq!(
        run_php(
            r#"<?php
echo addcslashes("az-A'\\", "a..z'\\"), '|';
$controls = chr(0).chr(7).chr(8).chr(9).chr(10).chr(11).chr(12).chr(13).chr(31).chr(127);
echo addcslashes($controls, chr(0).'..'.chr(127));
"#,
        ),
        "\\a\\z-A\\'\\\\|\\000\\a\\b\\t\\n\\v\\f\\r\\037\\177"
    );
}

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

#[test]
fn array_search_supports_strict_identity_matching() {
    assert_eq!(
        run_php(
            "<?php $values = ['loose' => '0', 'strict' => 0]; echo array_search(0, $values), '|'; echo array_search(0, $values, true);"
        ),
        "loose|strict"
    );
}

#[test]
fn debug_backtrace_reports_callers_arguments_limits_and_method_receivers() {
    assert_eq!(
        run_php(
            r#"<?php
function traceOuter($value) { traceInner($value); }
function traceInner($value) {
    $trace = debug_backtrace();
    echo $trace[0]['function'], ':', $trace[0]['args'][0], ':';
    echo $trace[1]['function'], ':', $trace[1]['args'][0], '|';
    $limited = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1);
    echo count($limited), ':', isset($limited[0]['args']) ? 'args' : 'ignored';
}
class TraceReceiver {
    public function probe() {
        $trace = debug_backtrace(DEBUG_BACKTRACE_PROVIDE_OBJECT, 1);
        echo '|', $trace[0]['class'], ':', $trace[0]['function'], ':', $trace[0]['type'], get_class($trace[0]['object']);
    }
}
traceOuter('payload');
(new TraceReceiver())->probe();
"#,
        ),
        "traceInner:payload:traceOuter:payload|1:ignored|TraceReceiver:probe:->TraceReceiver"
    );
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

#[test]
fn strpos_supports_positive_and_negative_offsets() {
    assert_eq!(
        run_php("<?php var_dump(strpos('abcabc', 'a', 1)); var_dump(strpos('abcabc', 'a', -4));"),
        "int(3)\nint(3)\n"
    );
}

#[test]
fn strrchr_returns_the_suffix_at_the_last_first_byte_match() {
    assert_eq!(
        run_php(
            "<?php echo strrchr('path/to/file.php', '/'), '|'; echo strrchr('abcabc', 'cd'), '|'; var_dump(strrchr('abc', ''));"
        ),
        "/file.php|c|bool(false)\n"
    );
}

// === strtr() ===

#[test]
fn test_e2e_strtr_character_map_is_simultaneous_and_truncates_to_shorter_map() {
    assert_eq!(
        run_php("<?php echo strtr('hello', 'el', 'ip'), '|'; echo strtr('abcd', 'abc', 'X');"),
        "hippo|Xbcd"
    );
}

#[test]
fn test_e2e_strtr_pair_map_prefers_longest_key_and_accepts_integer_keys() {
    assert_eq!(
        run_php(
            "<?php echo strtr('ababa', ['ab' => 'X', 'a' => 'Y']), '|'; echo strtr('123', [1 => 'x', '12' => 'y']);"
        ),
        "XXY|y3"
    );
}

#[test]
fn test_e2e_strtr_pair_map_warns_and_ignores_empty_keys() {
    assert_eq!(
        run_php("<?php echo strtr('abc', ['' => 'x']);"),
        "Warning: strtr(): Ignoring replacement of empty string\nabc"
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

#[test]
fn str_replace_supports_array_pairs_subject_keys_and_count() {
    assert_eq!(
        run_php(
            r#"<?php
$value = str_replace(['/','+'], ['.','_'], 'a/b+c', $count);
echo "$value|$count\n";
$values = str_replace(['a','b'], [2 => 'X', 1 => 'Y'], ['k' => 'aba', -1 => 'cab'], $count);
echo $values['k'], '|', $values[-1], '|', $count, "\n";
echo str_replace(['a', 'b'], ['b', 'c'], 'a'), "\n";
echo str_replace('', 'X', 'ab', $count), '|', $count;
"#,
        ),
        "a.b_c|2\nXYX|cXY|5\nc\nab|0"
    );
}

#[test]
fn str_replace_rejects_array_replacement_for_scalar_search() {
    assert_eq!(
        run_php(
            "<?php try { str_replace('a', ['X'], 'a'); } catch (TypeError $error) { echo $error->getMessage(); }"
        ),
        "str_replace(): Argument #2 ($replace) must be of type string when argument #1 ($search) is a string"
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

#[test]
fn test_e2e_trim_character_mask_and_range() {
    assert_eq!(
        run_php(
            "<?php echo ltrim('/route/', '/'), '|', rtrim('/route/', '/'), '|', trim('012abc210', '0..2');"
        ),
        "route/|/route|abc"
    );
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

#[test]
fn test_e2e_in_array_strict_does_not_coerce() {
    assert_eq!(
        run_php("<?php echo in_array(1, ['1'], true) ? 'bad' : 'strict';"),
        "strict"
    );
}

#[test]
fn test_filter_var_common_validators() {
    assert_eq!(
        run_php(
            "<?php echo filter_var('42', FILTER_VALIDATE_INT), ':'; echo filter_var('yes', FILTER_VALIDATE_BOOL) ? 'true' : 'false'; echo ':'; echo filter_var('127.0.0.1', FILTER_VALIDATE_IP, FILTER_FLAG_IPV4); echo ':'; echo filter_var('bad', FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE) === null ? 'null' : 'bad';"
        ),
        "42:true:127.0.0.1:null"
    );
}

#[test]
fn filter_var_validates_floats() {
    assert_eq!(
        run_php(
            "<?php echo filter_var('1.25', FILTER_VALIDATE_FLOAT), ':'; echo filter_var('not-a-float', FILTER_VALIDATE_FLOAT) === false ? 'false' : 'bad';"
        ),
        "1.25:false"
    );
}

#[test]
fn output_buffer_level_starts_at_zero() {
    assert_eq!(run_php("<?php echo ob_get_level();"), "0");
}

#[test]
fn get_debug_type_reports_scalar_and_object_names() {
    assert_eq!(
        run_php(
            "<?php class DebugTypeObject {} echo get_debug_type(null) . ':' . get_debug_type(1) . ':' . get_debug_type(new DebugTypeObject()) . ':' . get_debug_type(static fn () => null);"
        ),
        "null:int:DebugTypeObject:Closure"
    );
}

#[test]
fn substr_compare_supports_offsets_lengths_and_case_folding() {
    assert_eq!(
        run_php(
            "<?php echo substr_compare('FrameworkBundle', 'workbench', 5, 4), ':'; echo substr_compare('Symfony', 'SYMFONY', 0, null, true), ':'; echo substr_compare('abc', 'abd', 0), ':'; echo substr_compare('abc', 'a', -4);"
        ),
        "0:0:-1:1"
    );
}

#[test]
fn strnatcmp_orders_numeric_segments_like_php() {
    assert_eq!(
        run_php(
            "<?php $values = ['img10', 'img2', 'img02', 'img1']; usort($values, 'strnatcmp'); echo implode(',', $values), ':'; echo strnatcmp('02', '2'), ':', strnatcmp('1.10', '1.2');"
        ),
        "img02,img1,img2,img10:0:1"
    );
}

#[test]
fn string_comparisons_preserve_byte_differences_limits_and_ascii_case_folding() {
    assert_eq!(
        run_php(
            r#"<?php
echo strcmp("a", "d"), ":", strcmp("qwe", "qwer"), ":", strcmp("A\x00B", "A\x00c"), "\n";
echo strcasecmp("qwerty", "QweRty"), ":", strcasecmp("A\x00B", "a\x00c"), "\n";
echo strncmp("qwerty", "qwerty123", 6), ":", strncmp("qwerty", "qwerty123", 7), ":", strncmp("a", "d", 0), "\n";
echo strncasecmp("qwErtY", "qwer", 7), ":", strncasecmp("q123", "Q123", 3), "\n";
echo strcmp(string2: "b", string1: "a"), "\n";
foreach (["strncmp", "strncasecmp"] as $function) {
    try { $function("a", "b", -1); } catch (ValueError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        "-3:-1:-33\n0:-1\n0:-1:0\n1:0\n-1\nstrncmp(): Argument #3 ($length) must be greater than or equal to 0\nstrncasecmp(): Argument #3 ($length) must be greater than or equal to 0\n"
    );
}

#[test]
fn property_exists_sees_declared_static_inherited_trait_and_dynamic_properties() {
    assert_eq!(
        run_php(
            r#"<?php
trait TraitProperty { private $traitPrivate; }
class PropertyParent { private $parentPrivate; protected static $parentStatic; }
class PropertyChild extends PropertyParent { use TraitProperty; public int $uninitialized; }
$object = new PropertyChild;
$object->dynamic = 1;
foreach (["parentPrivate", "parentStatic", "traitPrivate", "uninitialized", "dynamic", "missing"] as $name) {
    var_dump(property_exists($object, $name));
}
unset($object->dynamic);
var_dump(property_exists($object, "dynamic"));
foreach (["parentPrivate", "parentStatic", "traitPrivate", "uninitialized", "missing"] as $name) {
    var_dump(property_exists("PropertyChild", $name));
}
var_dump(property_exists(function () {}, "anything"));
foreach ([[], 1, 3.5, true, null] as $invalid) {
    try { property_exists($invalid, "anything"); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "bool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(false)\n",
            "bool(false)\n",
            "bool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(false)\n",
            "bool(false)\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, array given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, int given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, float given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, bool given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, null given\n",
        )
    );
}

#[test]
fn incremental_xxh128_hash_matches_one_shot_hash() {
    assert_eq!(
        run_php(
            "<?php $context = hash_init('xxh128'); var_dump($context instanceof HashContext); hash_update($context, 'Symfony'); hash_update($context, '!'); echo hash_final($context), ':', hash('xxh128', 'Symfony!');"
        ),
        "bool(true)\n0290f3acc01ebe2ef48505b4a1179147:0290f3acc01ebe2ef48505b4a1179147"
    );
}

#[test]
fn array_diff_key_preserves_missing_keys_and_values() {
    assert_eq!(
        run_php(
            "<?php $result = array_diff_key(['keep' => 1, 'drop' => 2, 3 => 'three'], ['drop' => 9]); echo $result['keep'] . ':' . $result[3] . ':' . count($result);"
        ),
        "1:three:2"
    );
}

#[test]
fn array_key_set_operations_accept_variadic_comparison_arrays() {
    assert_eq!(
        run_php(
            "<?php $source = ['keep' => 1, 'shared' => 2, 3 => 'three']; $diff = array_diff_key($source, ['shared' => 9], [3 => 'other']); $intersect = array_intersect_key($source, ['keep' => 0, 'shared' => 0], ['shared' => 0, 3 => 0]); echo count($diff), ':', $diff['keep'], ':', count($intersect), ':', $intersect['shared'];"
        ),
        "1:1:1:2"
    );
}

#[test]
fn array_is_list_checks_ordered_keys_without_requiring_packed_storage() {
    assert_eq!(
        run_php(
            "<?php var_dump(array_is_list([])); var_dump(array_is_list([0 => 'a', 1 => 'b'])); var_dump(array_is_list([1 => 'a'])); $sparse = ['a', 'b', 'c']; unset($sparse[1]); var_dump(array_is_list($sparse));"
        ),
        "bool(true)\nbool(true)\nbool(false)\nbool(false)\n"
    );
}

#[test]
fn array_cursor_functions_track_reset_end_next_prev_current_and_key() {
    assert_eq!(
        run_php(
            "<?php $values = ['first' => 1, 'middle' => 2, 'last' => 3]; echo reset($values), ':', key($values), '|'; echo next($values), ':', key($values), ':', current($values), '|'; echo end($values), ':', key($values), '|'; echo prev($values), ':', key($values), '|'; $empty = []; var_dump(reset($empty));"
        ),
        "1:first|2:middle:2|3:last|2:middle|bool(false)\n"
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

#[test]
fn print_r_return_mode_returns_without_writing_output() {
    assert_eq!(
        run_php("<?php $result = print_r(['a' => 1, 2], true); var_dump($result);"),
        "string(36) \"Array\n(\n    [a] => 1\n    [0] => 2\n)\n\"\n"
    );
}

#[test]
fn enum_print_r_and_var_export_preserve_case_identity() {
    assert_eq!(
        run_php(
            r#"<?php
namespace Domain;
enum State: string { case Ready = "ready"; }
print_r(State::Ready);
echo var_export(State::Ready, true), "\n";
echo var_export([State::Ready], true), "\n";
"#
        ),
        "Domain\\State Enum:string\n(\n    [name] => Ready\n    [value] => ready\n)\n\\Domain\\State::Ready\narray (\n  0 =>\n  \\Domain\\State::Ready,\n)\n"
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

#[test]
fn test_e2e_gmdate_formats_http_date_in_utc() {
    assert_eq!(
        run_php("<?php echo gmdate('D, d M Y H:i:s T', 0);"),
        "Thu, 01 Jan 1970 00:00:00 GMT"
    );
}
#[test]
fn test_error_handler_api_is_available_for_warning_guards() {
    assert_eq!(
        run_php(
            "<?php function handleWarning($type, $message) { return true; } var_dump(set_error_handler('handleWarning')); var_dump(restore_error_handler());"
        ),
        "NULL\nbool(true)\n"
    );
}

#[test]
fn trigger_error_routes_allowed_levels_through_the_registered_handler() {
    assert_eq!(
        run_php(
            "<?php function handleUserError($level, $message, $file, $line) { echo $level, ':', $message, ':', strlen($file), ':', $line; return true; } set_error_handler('handleUserError', E_USER_WARNING); var_dump(trigger_error('careful', E_USER_WARNING)); var_dump(user_error('quiet', E_USER_NOTICE));"
        ),
        "512:careful:0:0bool(true)\nbool(true)\n"
    );
}

#[test]
fn trigger_error_rejects_non_user_error_levels() {
    assert_eq!(
        run_php(
            "<?php try { trigger_error('bad', E_WARNING); } catch (ValueError $error) { echo get_class($error); }"
        ),
        "ValueError"
    );
}

#[test]
fn is_iterable_accepts_arrays_and_traversable_objects_only() {
    assert_eq!(
        run_php(
            "<?php $storage = new SplObjectStorage(); echo is_iterable([]) ? 'array' : 'bad'; echo ':'; echo is_iterable($storage) ? 'object' : 'bad'; echo ':'; echo is_iterable(new stdClass()) ? 'bad' : 'plain'; echo ':'; echo is_iterable('text') ? 'bad' : 'scalar';"
        ),
        "array:object:plain:scalar"
    );
}

#[test]
fn json_unescaped_constants_and_flags_are_available() {
    assert_eq!(
        run_php(
            "<?php echo JSON_UNESCAPED_SLASHES, ':', JSON_UNESCAPED_UNICODE, '|'; echo json_encode(['path' => '/a/b', 'word' => 'Příliš'], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);"
        ),
        "64:256|{\"path\":\"/a/b\",\"word\":\"Příliš\"}"
    );
}

#[test]
fn gc_mem_caches_reports_no_zend_allocator_cache_and_supports_namespace_fallback() {
    assert_eq!(
        run_php("<?php namespace Fixture; var_dump(gc_mem_caches());"),
        "int(0)\n"
    );
}

#[test]
fn pathinfo_supports_component_flags_used_by_source_loaders() {
    assert_eq!(
        run_php(
            "<?php echo PATHINFO_ALL, ':'; echo pathinfo('/a/archive.tar.gz', PATHINFO_DIRNAME), ':'; echo pathinfo('/a/archive.tar.gz', PATHINFO_BASENAME), ':'; echo pathinfo('/a/archive.tar.gz', PATHINFO_EXTENSION), ':'; echo pathinfo('/a/archive.tar.gz', PATHINFO_FILENAME);"
        ),
        "15:/a:archive.tar.gz:gz:archive.tar"
    );
}
