mod common;

use common::run_php;

#[test]
fn json_encode_exposes_the_php_85_output_flag_constants() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'JSON_HEX_TAG', 'JSON_HEX_AMP', 'JSON_HEX_APOS', 'JSON_HEX_QUOT',
    'JSON_FORCE_OBJECT', 'JSON_NUMERIC_CHECK', 'JSON_UNESCAPED_SLASHES',
    'JSON_PRETTY_PRINT', 'JSON_UNESCAPED_UNICODE',
    'JSON_UNESCAPED_LINE_TERMINATORS',
] as $name) {
    echo $name, '=', constant($name), '|';
}
"#,
        ),
        concat!(
            "JSON_HEX_TAG=1|JSON_HEX_AMP=2|JSON_HEX_APOS=4|JSON_HEX_QUOT=8|",
            "JSON_FORCE_OBJECT=16|JSON_NUMERIC_CHECK=32|JSON_UNESCAPED_SLASHES=64|",
            "JSON_PRETTY_PRINT=128|JSON_UNESCAPED_UNICODE=256|",
            "JSON_UNESCAPED_LINE_TERMINATORS=2048|",
        )
    );
}

#[test]
fn json_encode_uses_php_default_slash_and_utf16_escaping() {
    assert_eq!(
        run_php(
            r#"<?php
echo json_encode(['/','é','𝄞', "\u{2027}", "\u{2028}", "\u{2029}"]), "\n";
"#,
        ),
        "[\"\\/\",\"\\u00e9\",\"\\ud834\\udd1e\",\"\\u2027\",\"\\u2028\",\"\\u2029\"]\n"
    );
}

#[test]
fn json_encode_applies_each_html_hex_flag_to_string_values_and_keys() {
    assert_eq!(
        run_php(
            r#"<?php
$value = ['<tag>' => ["'", '"', '&']];
foreach ([JSON_HEX_TAG, JSON_HEX_APOS, JSON_HEX_QUOT, JSON_HEX_AMP,
          JSON_HEX_TAG | JSON_HEX_APOS | JSON_HEX_QUOT | JSON_HEX_AMP] as $flags) {
    echo json_encode($value, $flags), "\n";
}
"#,
        ),
        concat!(
            "{\"\\u003Ctag\\u003E\":[\"'\",\"\\\"\",\"&\"]}\n",
            "{\"<tag>\":[\"\\u0027\",\"\\\"\",\"&\"]}\n",
            "{\"<tag>\":[\"'\",\"\\u0022\",\"&\"]}\n",
            "{\"<tag>\":[\"'\",\"\\\"\",\"\\u0026\"]}\n",
            "{\"\\u003Ctag\\u003E\":[\"\\u0027\",\"\\u0022\",\"\\u0026\"]}\n",
        )
    );
}

#[test]
fn json_encode_combines_unicode_slash_and_line_terminator_flags() {
    assert_eq!(
        run_php(
            r#"<?php
$value = ["/", "é", "𝄞", "\u{2027}", "\u{2028}", "\u{2029}"];
foreach ([
    JSON_UNESCAPED_UNICODE,
    JSON_UNESCAPED_SLASHES,
    JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_LINE_TERMINATORS,
    JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_LINE_TERMINATORS,
] as $flags) {
    echo json_encode($value, $flags), "\n";
}
"#,
        ),
        concat!(
            "[\"\\/\",\"é\",\"𝄞\",\"‧\",\"\\u2028\",\"\\u2029\"]\n",
            "[\"/\",\"\\u00e9\",\"\\ud834\\udd1e\",\"\\u2027\",\"\\u2028\",\"\\u2029\"]\n",
            "[\"\\/\",\"é\",\"𝄞\",\"‧\",\"\u{2028}\",\"\u{2029}\"]\n",
            "[\"/\",\"é\",\"𝄞\",\"‧\",\"\u{2028}\",\"\u{2029}\"]\n",
        )
    );
}

#[test]
fn json_encode_invalid_utf8_flags_repair_only_the_bad_sequences() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'a' . chr(255) . 'b',
    "a\xf0\x80\x80A",
    "a\xc0\x80A",
    "a\xc2\xc0A",
] as $value) {
    foreach ([0, JSON_INVALID_UTF8_IGNORE, JSON_INVALID_UTF8_SUBSTITUTE,
              JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE] as $flags) {
        $encoded = json_encode($value, $flags);
        echo $encoded === false ? 'F' : $encoded, ':', json_last_error(), '|';
    }
}
"#,
        ),
        concat!(
            "F:5|\"ab\":0|\"a\\ufffdb\":0|\"a�b\":0|",
            "F:5|\"aA\":0|\"a\\ufffdA\":0|\"a�A\":0|",
            "F:5|\"aA\":0|\"a\\ufffd\\ufffdA\":0|\"a��A\":0|",
            "F:5|\"aA\":0|\"a\\ufffdA\":0|\"a�A\":0|",
        )
    );
}

#[test]
fn json_numeric_check_projects_only_finite_complete_numeric_strings() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [
    '1', '1.0', '+1', '01', ' 1 ', '1e2', '-0.0', '1e20', '1.25', '1e-3',
    '9007199254740993.0', '1e400', '0x10',
];
echo json_encode($values, JSON_NUMERIC_CHECK), "\n";
$object = (object) ['1' => '5', 'plain' => '2.5'];
echo json_encode($object, JSON_NUMERIC_CHECK), "\n";
echo json_encode(['1.0', '-0.0'], JSON_NUMERIC_CHECK | JSON_PRESERVE_ZERO_FRACTION), "\n";
$invalid = "1\xff";
echo json_encode($invalid, JSON_NUMERIC_CHECK | JSON_INVALID_UTF8_IGNORE), '|';
echo json_encode($invalid, JSON_NUMERIC_CHECK | JSON_INVALID_UTF8_SUBSTITUTE), "\n";
"#,
        ),
        concat!(
            "[1,1,1,1,1,100,-0,1.0e+20,1.25,0.001,9007199254740992,\"1e400\",\"0x10\"]\n",
            "{\"1\":5,\"plain\":2.5}\n",
            "[1.0,-0.0]\n",
            "\"1\"|\"1\\ufffd\"\n",
        )
    );
}

#[test]
fn json_force_object_converts_every_nested_php_list() {
    assert_eq!(
        run_php(
            r#"<?php
$value = [[1], [], ['x' => 2]];
echo json_encode($value), "\n", json_encode($value, JSON_FORCE_OBJECT), "\n";
"#,
        ),
        "[[1],[],{\"x\":2}]\n{\"0\":{\"0\":1},\"1\":{},\"2\":{\"x\":2}}\n"
    );
}

#[test]
fn json_pretty_print_indents_nested_values_and_keeps_empty_containers_compact() {
    assert_eq!(
        run_php(
            r#"<?php
$value = ['a' => 1, 'b' => [2, []], 'c' => (object) []];
echo json_encode($value, JSON_PRETTY_PRINT), "\n";
"#,
        ),
        concat!(
            "{\n",
            "    \"a\": 1,\n",
            "    \"b\": [\n",
            "        2,\n",
            "        []\n",
            "    ],\n",
            "    \"c\": {}\n",
            "}\n",
        )
    );
}

#[test]
fn json_encode_preserves_declared_and_dynamic_property_order() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class Shape {
    public $z = 1;
    public $a = 2;
}
$shape = new Shape();
$shape->middle = 3;
echo json_encode($shape), "\n", json_encode($shape, JSON_PRETTY_PRINT), "\n";
$cast = (object) ['str' => 'first', 'int' => 2, 'arr' => []];
echo json_encode($cast), "\n";
"#,
        ),
        concat!(
            "{\"z\":1,\"a\":2,\"middle\":3}\n",
            "{\n",
            "    \"z\": 1,\n",
            "    \"a\": 2,\n",
            "    \"middle\": 3\n",
            "}\n",
            "{\"str\":\"first\",\"int\":2,\"arr\":[]}\n",
        )
    );
}

#[test]
fn partial_recursion_keeps_property_order_for_ordinary_and_custom_objects() {
    assert_eq!(
        run_php(
            r#"<?php
class OrdinaryLoop {
    public $first = 'ok';
    public $self;
    public function __construct() { $this->self = $this; }
}
class CustomLoop implements JsonSerializable {
    public $first = 'ok';
    public function jsonSerialize(): mixed {
        return ['first' => $this->first, 'self' => $this];
    }
}
echo json_encode(new OrdinaryLoop, JSON_PARTIAL_OUTPUT_ON_ERROR), '|';
echo json_encode(new CustomLoop, JSON_PARTIAL_OUTPUT_ON_ERROR), "\n";
"#,
        ),
        "{\"first\":\"ok\",\"self\":null}|{\"first\":\"ok\",\"self\":null}\n"
    );
}

#[test]
fn nested_json_encode_calls_do_not_share_pretty_formatter_state() {
    assert_eq!(
        run_php(
            r#"<?php
class NestedJson implements JsonSerializable {
    public function jsonSerialize(): mixed {
        return json_encode([1], JSON_PRETTY_PRINT);
    }
}
echo json_encode([new NestedJson]), '|', json_encode([1], JSON_PRETTY_PRINT), "\n";
"#,
        ),
        "[\"[\\n    1\\n]\"]|[\n    1\n]\n"
    );
}

#[test]
fn failed_encode_does_not_poison_the_next_pretty_formatter() {
    assert_eq!(
        run_php(
            r#"<?php
$stream = fopen('php://temp', 'r');
var_dump(json_encode(['stream' => $stream]));
echo json_last_error(), '|', json_encode([1], JSON_PRETTY_PRINT), ':', json_last_error();
"#,
        ),
        "bool(false)\n8|[\n    1\n]:0"
    );
}

#[test]
fn json_output_flags_work_through_named_dynamic_and_first_class_calls() {
    assert_eq!(
        run_php(
            r#"<?php
$encode = json_encode(...);
$dynamic = 'json_encode';
echo $encode(value: ['/' => 'é'], flags: JSON_UNESCAPED_SLASHES), '|';
echo $dynamic(...['value' => ['/' => 'é'], 'flags' => JSON_UNESCAPED_UNICODE]), '|';
echo call_user_func('json_encode', ['1'], JSON_NUMERIC_CHECK), '|';
echo json_encode('é/', JSON_THROW_ON_ERROR);
"#,
        ),
        "{\"/\":\"\\u00e9\"}|{\"\\/\":\"é\"}|[1]|\"\\u00e9\\/\""
    );
}
