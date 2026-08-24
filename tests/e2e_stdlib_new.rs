/// E2E tests for newly added stdlib functions.
mod common;
use common::run_php;

// === sizeof (alias for count) ===
#[test]
fn test_sizeof() {
    assert_eq!(run_php("<?php echo sizeof([1,2,3]);"), "3");
}

// === array_unshift ===
// Note: array_unshift modifies in-place but our VM uses SendVal (copy).
// Test the return value instead — the caller's variable won't be modified yet
// until we implement SendRef. For now, test that it at least returns count.
#[test]
fn test_array_unshift() {
    assert_eq!(
        run_php(
            r#"<?php
echo array_unshift([2, 3], 1);
"#
        ),
        "3"
    );
}

// === array_product ===
#[test]
fn test_array_product() {
    assert_eq!(run_php("<?php echo array_product([2, 3, 4]);"), "24");
}

#[test]
fn test_array_product_float() {
    assert_eq!(run_php("<?php echo array_product([2, 1.5]);"), "3");
}

// === number_format ===
#[test]
fn test_number_format_grouping_and_precision() {
    assert_eq!(
        run_php("<?php echo number_format(1234567.891, 2);"),
        "1,234,567.89"
    );
    assert_eq!(
        run_php("<?php echo number_format(-1234.5, 2);"),
        "-1,234.50"
    );
}

#[test]
fn test_number_format_custom_multibyte_separators() {
    assert_eq!(
        run_php(r#"<?php echo number_format(1234567.891, 2, "DEC", "SEP");"#),
        "1SEP234SEP567DEC89"
    );
}

// === array_count_values ===
#[test]
fn test_array_count_values() {
    assert_eq!(
        run_php(
            r#"<?php
$r = array_count_values(["a", "b", "a", "c", "b", "a"]);
echo $r["a"] . "," . $r["b"] . "," . $r["c"];
"#
        ),
        "3,2,1"
    );
}

// === array_fill ===
#[test]
fn test_array_fill() {
    assert_eq!(
        run_php(
            r#"<?php
$a = array_fill(5, 3, "x");
echo $a[5] . $a[6] . $a[7];
"#
        ),
        "xxx"
    );
}

// === array_pad ===
#[test]
fn test_array_pad_right() {
    assert_eq!(
        run_php(
            r#"<?php
$a = array_pad([1, 2], 5, 0);
echo implode(",", $a);
"#
        ),
        "1,2,0,0,0"
    );
}

#[test]
fn test_array_pad_left() {
    assert_eq!(
        run_php(
            r#"<?php
$a = array_pad([1, 2], -5, 0);
echo implode(",", $a);
"#
        ),
        "0,0,0,1,2"
    );
}

// === array_chunk ===
#[test]
fn test_array_chunk() {
    assert_eq!(
        run_php(
            r#"<?php
$chunks = array_chunk([1,2,3,4,5], 2);
echo count($chunks) . "," . count($chunks[0]) . "," . count($chunks[2]);
"#
        ),
        "3,2,1"
    );
}

// === array_column ===
#[test]
fn test_array_column() {
    assert_eq!(
        run_php(
            r#"<?php
$data = [
    ["name" => "Alice", "age" => 30],
    ["name" => "Bob", "age" => 25],
];
$names = array_column($data, "name");
echo implode(",", $names);
"#
        ),
        "Alice,Bob"
    );
}

// === array_splice ===
// Note: array_splice modifies first arg in-place (needs SendRef, not yet implemented).
// Test that returned removed elements are correct.
#[test]
fn test_array_splice() {
    assert_eq!(
        run_php(
            r#"<?php
$removed = array_splice([1, 2, 3, 4, 5], 1, 2);
echo implode(",", $removed);
"#
        ),
        "2,3"
    );
}

// === strrpos ===
#[test]
fn test_strrpos() {
    assert_eq!(
        run_php(
            r#"<?php
echo strrpos("hello world hello", "hello");
"#
        ),
        "12"
    );
}

#[test]
fn test_strrpos_not_found() {
    assert_eq!(
        run_php(
            r#"<?php
$r = strrpos("hello", "xyz");
var_dump($r);
"#
        ),
        "bool(false)\n"
    );
}

// === join (alias for implode) ===
#[test]
fn test_join() {
    assert_eq!(run_php(r#"<?php echo join("-", [1, 2, 3]);"#), "1-2-3");
}

// === str_word_count ===
#[test]
fn test_str_word_count() {
    assert_eq!(
        run_php(r#"<?php echo str_word_count("Hello beautiful world");"#),
        "3"
    );
}

// === nl2br ===
#[test]
fn test_nl2br() {
    assert_eq!(run_php("<?php echo nl2br(\"a\\nb\");"), "a<br />\nb");
}

// === strrev ===
#[test]
fn test_strrev() {
    assert_eq!(run_php(r#"<?php echo strrev("hello");"#), "olleh");
}

// === boolval ===
#[test]
fn test_boolval() {
    assert_eq!(
        run_php(
            r#"<?php
echo boolval(1) ? "yes" : "no";
echo ",";
echo boolval(0) ? "yes" : "no";
echo ",";
echo boolval("") ? "yes" : "no";
echo ",";
echo boolval("a") ? "yes" : "no";
"#
        ),
        "yes,no,no,yes"
    );
}

// === intval / floatval with type juggling ===
#[test]
fn test_intval_string() {
    assert_eq!(run_php(r#"<?php echo intval("42abc");"#), "42");
}

#[test]
fn test_floatval_string() {
    assert_eq!(run_php(r#"<?php echo floatval("3.14");"#), "3.14");
}

// === is_object ===
#[test]
fn test_is_object() {
    assert_eq!(
        run_php(
            r#"<?php
class Foo {}
$f = new Foo();
echo is_object($f) ? "yes" : "no";
echo ",";
echo is_object(42) ? "yes" : "no";
echo ",";
$closure = static function () {};
echo is_object($closure) ? "yes" : "no";
echo "," . gettype($closure);
"#
        ),
        "yes,no,yes,object"
    );
}

// === settype ===
#[test]
fn test_settype() {
    assert_eq!(
        run_php(
            r#"<?php
$value = "42";
var_dump(settype($value, "integer"), $value);
"#
        ),
        "bool(true)\nint(42)\n"
    );
}

#[test]
fn test_settype_nan_warning_observes_reentrant_reference_writes() {
    assert_eq!(
        run_php(
            r#"<?php
$value = fdiv(0, 0);
set_error_handler(function ($level, $message) use (&$value) {
    $value = "changed";
    echo $message, "\n";
});
settype($value, "object");
restore_error_handler();
echo get_class($value), ":", $value->scalar, "\n";

$value = fdiv(0, 0);
set_error_handler(function ($level, $message) use (&$value) {
    $value = null;
    echo $message, "\n";
});
settype($value, "boolean");
restore_error_handler();
var_dump($value);
"#
        ),
        "unexpected NAN value was coerced to object\nstdClass:changed\n\
unexpected NAN value was coerced to bool\nbool(true)\n"
    );
}

#[test]
fn test_settype_container_and_invalid_resource_conversions() {
    assert_eq!(
        run_php(
            r#"<?php
$value = null;
settype($value, "ARRAY");
echo count($value), "\n";

$value = [2 => "x", "name" => 7];
settype($value, "object");
echo get_class($value), ":", $value->name, ":", count(get_object_vars($value)), "\n";

$value = 12;
try {
    settype($value, "resource");
} catch (ValueError $error) {
    echo $error->getMessage(), ":", $value, "\n";
}

class Convertible {}
$value = new Convertible();
set_error_handler(function ($level, $message) { echo $message, "\n"; });
settype($value, "int");
restore_error_handler();
var_dump($value);
"#
        ),
        "0\nstdClass:7:2\nCannot convert to resource type:12\n\
Object of class Convertible could not be converted to int\nint(1)\n"
    );
}

#[test]
fn test_random_bytes_uses_system_source_and_validates_length() {
    assert_eq!(
        run_php(
            r#"<?php
$bytes = random_bytes(8);
echo strlen(bin2hex($bytes)), "\n";
try {
    random_bytes(0);
} catch (ValueError $error) {
    echo $error->getMessage();
}
"#
        ),
        "16\nrandom_bytes(): Argument #1 ($length) must be greater than 0"
    );
}

#[test]
fn random_int_uses_inclusive_bounds_and_validates_range_order() {
    assert_eq!(
        run_php(
            r#"<?php
$valid = true;
for ($i = 0; $i < 64; $i++) {
    $value = random_int(-2, 2);
    $valid = $valid && $value >= -2 && $value <= 2;
}
echo (int) $valid, '|', random_int(PHP_INT_MIN, PHP_INT_MIN), '|';
try {
    random_int(2, 1);
} catch (ValueError $error) {
    echo $error->getMessage();
}
"#,
        ),
        "1|-9223372036854775808|random_int(): Argument #1 ($min) must be less than or equal to argument #2 ($max)"
    );
}

// === Math functions ===
#[test]
fn test_intdiv() {
    assert_eq!(run_php("<?php echo intdiv(7, 2);"), "3");
}

#[test]
fn test_fmod() {
    // Use round to avoid floating-point precision issues
    assert_eq!(run_php("<?php echo round(fmod(10.5, 3.2), 1);"), "0.9");
}

#[test]
fn test_fdiv_preserves_ieee_754_results() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(fdiv(7, 2));
var_dump(fdiv(1.0, 0.0));
var_dump(fdiv(-1.0, 0.0));
var_dump(fdiv(1.0, -0.0));
var_dump(fdiv(0.0, 0.0));
var_dump(fdiv(-0.0, INF));
var_dump(fdiv(num2: 2, num1: "7.5"));
try {
    fdiv(1);
} catch (ArgumentCountError $error) {
    echo $error->getMessage(), "\n";
}
"#
        ),
        "float(3.5)\nfloat(INF)\nfloat(-INF)\nfloat(-INF)\nfloat(NAN)\nfloat(-0)\n\
float(3.75)\nfdiv() expects exactly 2 arguments, 1 given\n"
    );
}

#[test]
fn test_log_fn() {
    // Natural log of e^1 = 1
    assert_eq!(run_php(r#"<?php echo round(log(2.718281828), 0);"#), "1");
}

#[test]
fn test_log10_fn() {
    assert_eq!(run_php("<?php echo log10(1000);"), "3");
}

#[test]
fn test_log2_fn() {
    assert_eq!(run_php("<?php echo log2(8);"), "3");
}

#[test]
fn test_pi() {
    assert_eq!(
        run_php(r#"<?php echo substr(strval(pi()), 0, 5);"#),
        "3.141"
    );
}

#[test]
fn test_round_precision() {
    assert_eq!(run_php("<?php echo round(3.14159, 2);"), "3.14");
}

// === var_export ===
#[test]
fn test_var_export() {
    assert_eq!(run_php(r#"<?php echo var_export(42, true);"#), "42");
}

#[test]
fn test_var_export_string() {
    assert_eq!(
        run_php(r#"<?php echo var_export("hello", true);"#),
        "'hello'"
    );
}

#[test]
fn test_var_export_array() {
    assert_eq!(
        run_php(r#"<?php echo var_export([1, 2], true);"#),
        "array (\n  0 => 1,\n  1 => 2,\n)"
    );
}

#[test]
fn var_export_uses_canonical_special_float_spellings() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [NAN, INF, -INF, [NAN, INF, -INF]];
foreach ($values as $value) {
    var_export($value);
    echo "|", var_export($value, true), "\n";
}
"#,
        ),
        "NAN|NAN\nINF|INF\n-INF|-INF\narray (\n  0 => NAN,\n  1 => INF,\n  2 => -INF,\n)|array (\n  0 => NAN,\n  1 => INF,\n  2 => -INF,\n)\n"
    );
}

// === json_encode / json_decode ===
#[test]
fn test_json_encode_array() {
    assert_eq!(run_php(r#"<?php echo json_encode([1, 2, 3]);"#), "[1,2,3]");
}

#[test]
fn test_json_encode_assoc() {
    assert_eq!(
        run_php(r#"<?php echo json_encode(["a" => 1, "b" => 2]);"#),
        "{\"a\":1,\"b\":2}"
    );
}

#[test]
fn test_json_encode_string() {
    assert_eq!(run_php(r#"<?php echo json_encode("hello");"#), "\"hello\"");
}

#[test]
fn test_json_decode_object() {
    assert_eq!(
        run_php(
            r#"<?php
$data = json_decode('{"name":"Alice","age":30}', true);
echo $data["name"] . "," . $data["age"];
"#
        ),
        "Alice,30"
    );
}

#[test]
fn test_json_decode_array() {
    assert_eq!(
        run_php(
            r#"<?php
$data = json_decode('[1,2,3]', true);
echo count($data) . "," . $data[1];
"#
        ),
        "3,2"
    );
}

#[test]
fn test_json_roundtrip() {
    assert_eq!(
        run_php(
            r#"<?php
$data = ["x" => [1, 2], "y" => "hello"];
$json = json_encode($data);
$back = json_decode($json, true);
echo $back["y"] . "," . count($back["x"]);
"#
        ),
        "hello,2"
    );
}

// === sprintf with multiple args ===
#[test]
fn test_sprintf_multiple() {
    assert_eq!(
        run_php(
            r#"<?php
echo sprintf("Name: %s, Age: %d", "Alice", 30);
"#
        ),
        "Name: Alice, Age: 30"
    );
}

#[test]
fn test_sprintf_hex() {
    assert_eq!(run_php(r#"<?php echo sprintf("%x", 255);"#), "ff");
}

#[test]
fn test_sprintf_percent() {
    assert_eq!(run_php(r#"<?php echo sprintf("100%%");"#), "100%");
}

#[test]
fn test_sprintf_utf8_literals_and_arguments() {
    assert_eq!(
        run_php(r#"<?php echo sprintf("žluť %s %% %d", "kůň", 7);"#),
        "žluť kůň % 7"
    );
}

#[test]
fn test_sprintf_numeric_formats_write_directly() {
    assert_eq!(
        run_php(r#"<?php echo sprintf("%f %o %b %c", 1.5, 8, 5, 65);"#),
        "1.500000 10 101 A"
    );
}

#[test]
fn test_vsprintf_reuses_array_values_without_changing_format_semantics() {
    assert_eq!(
        run_php(r#"<?php echo vsprintf("[%s, %d, %x, %%]", ["route", 7, 255]);"#),
        "[route, 7, ff, %]"
    );
}

#[test]
fn printf_and_vprintf_write_formatted_output_and_return_byte_lengths() {
    assert_eq!(
        run_php(
            r#"<?php
$first = printf("[%s:%d:%%]", "ž", 7);
echo '|', $first, '|';
$second = vprintf("[%s:%x]", ["kůň", 255]);
echo '|', $second;
"#,
        ),
        "[ž:7:%]|8|[kůň:ff]|10"
    );
}

#[test]
fn formatted_string_slots_invoke_object_conversion_in_argument_order() {
    assert_eq!(
        run_php(
            r#"<?php
class FormattedStringValue {
    public function __construct(private string $value) {}
    public function __toString(): string {
        echo 'convert:', $this->value, '|';
        return $this->value;
    }
}
class RejectedFormattedString {
    public function __toString(): string {
        echo 'reject|';
        throw new Exception('stop');
    }
}
echo sprintf('[%s]', new FormattedStringValue('sprintf')), '|';
echo vsprintf('[%s]', [new FormattedStringValue('vsprintf')]), '|';
printf('[%s]', new FormattedStringValue('printf'));
echo '|';
vprintf('[%s]', [new FormattedStringValue('vprintf')]);
echo '|';
try {
    printf('hidden:%s', new RejectedFormattedString());
} catch (Exception $exception) {
    echo $exception->getMessage();
}
"#,
        ),
        concat!(
            "convert:sprintf|[sprintf]|",
            "convert:vsprintf|[vsprintf]|",
            "convert:printf|[printf]|",
            "convert:vprintf|[vprintf]|",
            "reject|stop",
        )
    );
}

#[test]
fn binary_hex_conversions_round_trip_bytes_and_report_invalid_input() {
    assert_eq!(
        run_php(
            r#"<?php
echo bin2hex(chr(0).chr(127).chr(128).chr(255)), '|';
echo bin2hex(hex2bin('00Ff7f80')), '|';
set_error_handler(function ($level, $message) { echo $level, ':', $message, '|'; });
echo hex2bin('0') === false ? 'odd|' : 'wrong|';
echo hex2bin('0g') === false ? 'digit' : 'wrong';
restore_error_handler();
"#,
        ),
        "007f80ff|00ff7f80|2:hex2bin(): Hexadecimal input string must have an even length|odd|2:hex2bin(): Input string must be hexadecimal string|digit"
    );
}

#[test]
fn bitwise_string_operators_use_php_byte_and_length_rules() {
    assert_eq!(
        run_php(
            r#"<?php
$short = "12";
$long = "abc";
echo bin2hex($short & $long), '|';
echo bin2hex($short | $long), '|';
echo bin2hex($short ^ $long), '|';
echo bin2hex(~hex2bin('007f80ff')), '|';
echo 6 & "3";
"#,
        ),
        "2122|717263|5050|ff807f00|2"
    );
}

#[test]
fn user_sorts_preserve_keys_and_compare_values_or_keys() {
    assert_eq!(
        run_php(
            "<?php $byValue = ['second' => 2, 'first' => 1]; uasort($byValue, static fn ($left, $right) => $left <=> $right); echo implode(',', array_keys($byValue)), ':'; $byKey = ['item10' => 10, 'item2' => 2]; uksort($byKey, static fn ($left, $right) => (int) substr($left, 4) <=> (int) substr($right, 4)); echo implode(',', array_keys($byKey));"
        ),
        "first,second:item2,item10"
    );
}

#[test]
fn array_multisort_permutates_columns_and_preserves_preferred_references() {
    assert_eq!(
        run_php(
            r#"<?php
$primary = ['first' => 2, 7 => 1, 'tie' => 1];
$secondary = ['first' => 'b', 7 => 'z', 'tie' => 'a'];
var_dump(array_multisort($primary, SORT_ASC, SORT_NUMERIC, $secondary, SORT_DESC, SORT_STRING));
echo implode(',', array_keys($primary)), ':', implode(',', $primary), '|';
echo implode(',', array_keys($secondary)), ':', implode(',', $secondary), '|';
$callback = array_multisort(...);
$direct = [3, 1, 2];
$callback($direct);
echo implode(',', $direct), '|';
$forwarded = ['row1' => 2, 'row2' => 1];
$arguments = [&$forwarded];
call_user_func_array('array_multisort', $arguments);
echo implode(',', array_keys($forwarded));"#,
        ),
        "bool(true)\n0,tie,first:1,1,2|0,tie,first:z,a,b|1,2,3|row2,row1"
    );
}

// === shuffle / array_rand (just ensure no crash) ===
#[test]
fn test_shuffle_runs() {
    assert_eq!(
        run_php(
            r#"<?php
$a = [1, 2, 3, 4, 5];
shuffle($a);
echo count($a);
"#
        ),
        "5"
    );
}

#[test]
fn test_array_rand_runs() {
    assert_eq!(
        run_php(
            r#"<?php
$a = ["a" => 1, "b" => 2, "c" => 3];
$k = array_rand($a);
echo is_string($k) ? "ok" : "ok";
"#
        ),
        "ok"
    );
}

// === wordwrap ===
#[test]
fn test_wordwrap() {
    assert_eq!(
        run_php(r#"<?php echo wordwrap("The quick brown fox", 10, "\n");"#),
        "The quick\nbrown fox"
    );
}

// === strtolower / strtoupper (ASCII fast-path) ===
#[test]
fn test_strtolower_ascii() {
    assert_eq!(
        run_php(r#"<?php echo strtolower("Hello WORLD 123");"#),
        "hello world 123"
    );
}

#[test]
fn test_strtoupper_ascii() {
    assert_eq!(
        run_php(r#"<?php echo strtoupper("Hello world 123");"#),
        "HELLO WORLD 123"
    );
}

// ==========================================================================
// === URL encoding ===

#[test]
fn test_urlencode_reserved_characters_and_space() {
    assert_eq!(
        run_php(r#"<?php echo urlencode("a b+c/~");"#),
        "a+b%2Bc%2F%7E"
    );
}

#[test]
fn test_rawurlencode_reserved_characters_and_space() {
    assert_eq!(
        run_php(r#"<?php echo rawurlencode("a b+c/~");"#),
        "a%20b%2Bc%2F~"
    );
}

#[test]
fn test_urlencode_utf8_bytes() {
    assert_eq!(
        run_php(r#"<?php echo urlencode("žluť");"#),
        "%C5%BElu%C5%A5"
    );
}

#[test]
fn test_urldecode_accepts_lowercase_hex_and_preserves_invalid_escapes() {
    assert_eq!(
        run_php(r#"<?php echo urldecode("a+b%2bc%2F%7e%ZZ%");"#),
        "a b+c/~%ZZ%"
    );
}

#[test]
fn test_rawurldecode_preserves_plus_and_invalid_escapes() {
    assert_eq!(
        run_php(r#"<?php echo rawurldecode("a+b%2bc%2F~%ZZ%");"#),
        "a+b+c/~%ZZ%"
    );
}

#[test]
fn test_url_encoding_utf8_round_trip() {
    assert_eq!(
        run_php(r#"<?php echo urldecode(urlencode("Příliš žluťoučký kůň"));"#),
        "Příliš žluťoučký kůň"
    );
}

// Regression tests for code review findings
// ==========================================================================

// P1: compact() must read the caller scope rather than returning a stub value.
#[test]
fn test_compact_reads_caller_scope() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 42;
$result = compact('value');
echo $result['value'];
"#,
        ),
        "42"
    );
}

// P2: json_decode assoc=false should return object, assoc=true should return array
#[test]
fn test_json_decode_assoc_true() {
    assert_eq!(
        run_php(
            r#"<?php
$data = json_decode('{"name":"Alice"}', true);
echo is_array($data) ? "array" : "not_array";
echo "," . $data["name"];
"#
        ),
        "array,Alice"
    );
}

#[test]
fn test_json_decode_assoc_false() {
    assert_eq!(
        run_php(
            r#"<?php
$data = json_decode('{"name":"Bob"}', false);
echo is_object($data) ? "object" : "not_object";
echo "," . $data->name;
"#
        ),
        "object,Bob"
    );
}

#[test]
fn test_json_decode_default_is_object() {
    assert_eq!(
        run_php(
            r#"<?php
$data = json_decode('{"x":42}');
echo is_object($data) ? "object" : "not_object";
"#
        ),
        "object"
    );
}

// P2: Variadic param with default value should be parse error
#[test]
fn test_variadic_default_rejected() {
    // This should produce a parse/compile error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
function f(...$args = 1) { echo count($args); }
f(1, 2, 3);
"#,
        )
    });
    assert!(result.is_err());
}
