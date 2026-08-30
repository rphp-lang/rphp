mod common;

use common::run_php;

#[test]
fn json_functions_expose_php_85_signatures_defaults_and_named_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['json_encode', 'json_decode'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, '|', $reflection->getNumberOfParameters(), '|',
        $reflection->getNumberOfRequiredParameters(), '|',
        (string) $reflection->getReturnType(), "\n";
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), '|', (string) $parameter->getType(), '|',
            $parameter->isOptional() ? 1 : 0, '|',
            $parameter->isDefaultValueAvailable()
                ? var_export($parameter->getDefaultValue(), true)
                : '-', '|',
            $parameter->allowsNull() ? 1 : 0, '|',
            $parameter->getType()->allowsNull() ? 1 : 0,
            "\n";
    }
}
foreach (['json_last_error', 'json_last_error_msg'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, '|', $reflection->getNumberOfParameters(), '|',
        $reflection->getNumberOfRequiredParameters(), '|',
        (string) $reflection->getReturnType(), "\n";
}
echo json_encode(value: ['x' => 1], depth: 2), "\n";
var_dump(json_decode(json: '{"x":1}', flags: JSON_OBJECT_AS_ARRAY));
"#,
        ),
        concat!(
            "json_encode|3|1|string|false\n",
            "value|mixed|0|-|1|1\n",
            "flags|int|1|0|0|0\n",
            "depth|int|1|512|0|0\n",
            "json_decode|4|1|mixed\n",
            "json|string|0|-|0|0\n",
            "associative|?bool|1|NULL|1|1\n",
            "depth|int|1|512|0|0\n",
            "flags|int|1|0|0|0\n",
            "json_last_error|0|0|int\n",
            "json_last_error_msg|0|0|string\n",
            "{\"x\":1}\n",
            "array(1) {\n",
            "  [\"x\"]=>\n",
            "  int(1)\n",
            "}\n",
        )
    );
}

#[test]
fn decode_associative_argument_precedes_object_as_array_flag() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"x":1}';
foreach ([null, true, false] as $associative) {
    foreach ([0, JSON_OBJECT_AS_ARRAY] as $flags) {
        $value = json_decode($json, $associative, 512, $flags);
        echo var_export($associative, true), '/', $flags, '|',
            get_debug_type($value), '|',
            is_array($value) ? $value['x'] : $value->x, '|',
            json_last_error(), "\n";
    }
}
"#,
        ),
        concat!(
            "NULL/0|stdClass|1|0\n",
            "NULL/1|array|1|0\n",
            "true/0|array|1|0\n",
            "true/1|array|1|0\n",
            "false/0|stdClass|1|0\n",
            "false/1|stdClass|1|0\n",
        )
    );
}

#[test]
fn decode_bigint_flag_preserves_only_out_of_range_integer_lexemes() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    '9223372036854775807',
    '9223372036854775808',
    '-9223372036854775808',
    '-9223372036854775809',
    '18446744073709551616',
    '999999999999999999999999999999',
] as $json) {
    $value = json_decode($json, null, 512, JSON_BIGINT_AS_STRING);
    echo get_debug_type($value), '|', $value, '|', json_last_error(), "\n";
}

"#,
        ),
        concat!(
            "int|9223372036854775807|0\n",
            "string|9223372036854775808|0\n",
            "int|-9223372036854775808|0\n",
            "string|-9223372036854775809|0\n",
            "string|18446744073709551616|0\n",
            "string|999999999999999999999999999999|0\n",
        )
    );
}

#[test]
fn decode_numeric_overflow_matches_php_infinity_projection() {
    assert_eq!(
        run_php(
            r#"<?php
$value = json_decode('[1e400,-1e400,-0,-0.0,-0e0]');
foreach ($value as $number) {
    echo get_debug_type($number), '|', var_export($number, true), '|',
        json_last_error(), "\n";
}
"#,
        ),
        concat!(
            "float|INF|0\n",
            "float|-INF|0\n",
            "int|0|0\n",
            "float|-0.0|0\n",
            "float|-0.0|0\n",
        )
    );
}

#[test]
fn decode_depth_counts_containers_and_crosses_the_serde_default_limit() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    ['scalar', '1', 1],
    ['empty-one', '[]', 1],
    ['empty-two', '[]', 2],
    ['nested-two', '[[1]]', 2],
    ['nested-three', '[[1]]', 3],
];
foreach ($cases as [$label, $json, $depth]) {
    $value = json_decode($json, true, $depth);
    echo $label, '|', get_debug_type($value), '|', json_last_error(), "\n";
}
$deep = str_repeat('[', 130) . '0' . str_repeat(']', 130);
foreach ([130, 131, 512] as $depth) {
    $value = json_decode($deep, true, $depth);
    echo 'deep-', $depth, '|', get_debug_type($value), '|', json_last_error(), "\n";
}
"#,
        ),
        concat!(
            "scalar|int|0\n",
            "empty-one|null|1\n",
            "empty-two|array|0\n",
            "nested-two|null|1\n",
            "nested-three|array|0\n",
            "deep-130|null|1\n",
            "deep-131|array|0\n",
            "deep-512|array|0\n",
        )
    );
}

#[test]
fn decode_depth_rejects_values_outside_the_php_integer_domain() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([0, -1, 2147483647, 2147483648] as $depth) {
    echo $depth, '|';
    try {
        $value = json_decode('[]', true, $depth);
        echo get_debug_type($value), '|', json_last_error(), "\n";
    } catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), '|', json_last_error(), "\n";
    }
}
"#,
        ),
        concat!(
            "0|ValueError|json_decode(): Argument #3 ($depth) must be greater than 0|0\n",
            "-1|ValueError|json_decode(): Argument #3 ($depth) must be greater than 0|0\n",
            "2147483647|array|0\n",
            "2147483648|ValueError|json_decode(): Argument #3 ($depth) must be less than 2147483647|0\n",
        )
    );
}

#[test]
fn decode_utf8_flags_repair_only_bytes_inside_json_strings() {
    assert_eq!(
        run_php(
            r#"<?php
$inside = "\"A\xFFB\"";
$outside = "[\xFF]";
foreach ([
    0,
    JSON_INVALID_UTF8_IGNORE,
    JSON_INVALID_UTF8_SUBSTITUTE,
    JSON_INVALID_UTF8_IGNORE | JSON_INVALID_UTF8_SUBSTITUTE,
] as $flags) {
    $value = json_decode($inside, null, 512, $flags);
    echo $flags, '|', get_debug_type($value), '|',
        is_string($value) ? bin2hex($value) : '-', '|', json_last_error(), "\n";
}
foreach ([JSON_INVALID_UTF8_IGNORE, JSON_INVALID_UTF8_SUBSTITUTE] as $flags) {
    $value = json_decode($outside, null, 512, $flags);
    echo 'outside-', $flags, '|', get_debug_type($value), '|', json_last_error(), "\n";
}
"#,
        ),
        concat!(
            "0|null|-|5\n",
            "1048576|string|4142|0\n",
            "2097152|string|41efbfbd42|0\n",
            "3145728|string|41efbfbd42|0\n",
            "outside-1048576|null|5\n",
            "outside-2097152|null|5\n",
        )
    );
}

#[test]
fn decode_throw_flag_preserves_last_error_and_reports_specific_codes() {
    assert_eq!(
        run_php(
            r#"<?php
json_decode('{');
echo 'seed=', json_last_error(), ';';
$value = json_decode('{}', null, 512, JSON_THROW_ON_ERROR);
echo 'success=', get_debug_type($value), ':', json_last_error(), ';';
foreach (['[}', "\"a\x01b\"", '"\uD800"', '"a\"\uD800"', '"a\"\uDC00"', '{"\u0000x":1}'] as $json) {
    try {
        json_decode($json, null, 512, JSON_THROW_ON_ERROR);
    } catch (Throwable $error) {
        echo $error->getCode(), ':', $error->getMessage(), ':', json_last_error(), ';';
    }
}
"#,
        ),
        concat!(
            "seed=4;success=stdClass:4;",
            "2:State mismatch (invalid or malformed JSON):4;",
            "3:Control character error, possibly incorrectly encoded:4;",
            "10:Single unpaired UTF-16 surrogate in unicode escape:4;",
            "10:Single unpaired UTF-16 surrogate in unicode escape:4;",
            "10:Single unpaired UTF-16 surrogate in unicode escape:4;",
            "9:The decoded property name is invalid:4;",
        )
    );
}

#[test]
fn decode_invalid_property_names_depend_on_object_materialization() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"\u0000x":1}';
foreach ([null, true] as $associative) {
    $value = json_decode($json, $associative);
    echo var_export($associative, true), '|', get_debug_type($value), '|',
        json_last_error();
    if (is_array($value)) {
        echo '|', bin2hex(array_keys($value)[0]);
    }
    echo "\n";
}
"#,
        ),
        "NULL|null|9\ntrue|array|0|0078\n"
    );
}

#[test]
fn encode_depth_partial_and_throw_modes_share_php_error_state() {
    assert_eq!(
        run_php(
            r#"<?php
$value = ['a' => ['b' => ['c' => 1]], 'z' => 2];
foreach ([[1, 0], [2, 0], [3, 0], [1, JSON_PARTIAL_OUTPUT_ON_ERROR], [0, JSON_PARTIAL_OUTPUT_ON_ERROR]] as [$depth, $flags]) {
    $result = json_encode($value, $flags, $depth);
    echo $depth, '/', $flags, '|', get_debug_type($result), '|',
        $result === false ? '-' : $result, '|', json_last_error(), "\n";
}
echo 'scalar|', json_encode(1, 0, 0), '|', json_last_error(), "\n";
json_encode(NAN);
echo 'seed=', json_last_error(), ';';
try {
    json_encode($value, JSON_THROW_ON_ERROR, 1);
} catch (Throwable $error) {
    echo 'throw=', $error->getCode(), ':', $error->getMessage(), ':', json_last_error(), ';';
}
echo 'partial=', json_encode(
    $value,
    JSON_THROW_ON_ERROR | JSON_PARTIAL_OUTPUT_ON_ERROR,
    1,
), ':', json_last_error();
echo "\n";
foreach ([
    ['nan', [NAN], 0],
    ['nan-partial', [NAN], JSON_PARTIAL_OUTPUT_ON_ERROR],
    ['utf8', ["\xFF"], 0],
    ['utf8-partial', ["\xFF"], JSON_PARTIAL_OUTPUT_ON_ERROR],
] as [$label, $input, $flags]) {
    $result = json_encode($input, $flags, 0);
    echo $label, '|', get_debug_type($result), '|',
        $result === false ? '-' : bin2hex($result), '|', json_last_error(), "\n";
}
"#,
        ),
        concat!(
            "1/0|bool|-|1\n",
            "2/0|bool|-|1\n",
            "3/0|string|{\"a\":{\"b\":{\"c\":1}},\"z\":2}|0\n",
            "1/512|string|{\"a\":{\"b\":{\"c\":1}},\"z\":2}|1\n",
            "0/512|string|{\"a\":{\"b\":{\"c\":1}},\"z\":2}|1\n",
            "scalar|1|0\n",
            "seed=7;throw=1:Maximum stack depth exceeded:7;",
            "partial={\"a\":{\"b\":{\"c\":1}},\"z\":2}:1\n",
            "nan|bool|-|1\n",
            "nan-partial|string|5b305d|1\n",
            "utf8|bool|-|5\n",
            "utf8-partial|string|5b6e756c6c5d|1\n",
        )
    );
}

#[test]
fn json_depth_counts_serialized_and_ordinary_object_containers_equally() {
    assert_eq!(
        run_php(
            r#"<?php
class Encoded implements JsonSerializable {
    public function jsonSerialize(): mixed { return ['a' => ['b' => 1]]; }
}
class ThrowEncoded implements JsonSerializable {
    public function jsonSerialize(): mixed {
        echo 'callback|';
        throw new Exception('stop');
    }
}
$values = [
    ['a' => ['b' => 1]],
    (object) ['a' => (object) ['b' => 1]],
    new Encoded,
];
foreach ($values as $index => $value) {
    foreach ([1, 2] as $depth) {
        $result = json_encode($value, 0, $depth);
        echo $index, '/', $depth, '|', get_debug_type($result), '|',
            $result === false ? '-' : $result, '|', json_last_error(), "\n";
    }
}
$resource = fopen('php://memory', 'r');
$result = json_encode([[$resource]], 0, 1);
echo 'resource|', get_debug_type($result), '|', json_last_error(), "\n";
fclose($resource);
try {
    json_encode([[new ThrowEncoded]], 0, 1);
} catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "0/1|bool|-|1\n",
            "0/2|string|{\"a\":{\"b\":1}}|0\n",
            "1/1|bool|-|1\n",
            "1/2|string|{\"a\":{\"b\":1}}|0\n",
            "2/1|bool|-|1\n",
            "2/2|string|{\"a\":{\"b\":1}}|0\n",
            "resource|bool|8\n",
            "callback|Exception|stop\n",
        )
    );
}

#[test]
fn json_parameters_follow_weak_conversion_and_diagnostic_order() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($number, $message) {
    echo 'diag|', $number, '|', $message, "\n";
    return true;
});
$cases = [
    fn() => json_encode([1], '0', '2'),
    fn() => json_encode([1], null, null),
    fn() => json_decode(123, 1, '2', '1'),
    fn() => json_decode(null, null, null, null),
];
foreach ($cases as $index => $case) {
    echo 'case-', $index, '|';
    try {
        var_dump($case());
    } catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "case-0|string(3) \"[1]\"\n",
            "case-1|diag|8192|json_encode(): Passing null to parameter #2 ($flags) of type int is deprecated\n",
            "diag|8192|json_encode(): Passing null to parameter #3 ($depth) of type int is deprecated\n",
            "bool(false)\n",
            "case-2|int(123)\n",
            "case-3|diag|8192|json_decode(): Passing null to parameter #1 ($json) of type string is deprecated\n",
            "diag|8192|json_decode(): Passing null to parameter #3 ($depth) of type int is deprecated\n",
            "diag|8192|json_decode(): Passing null to parameter #4 ($flags) of type int is deprecated\n",
            "NULL\n",
        )
    );
}

#[test]
fn json_parameters_reject_scalar_coercion_in_strict_calls() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
$cases = [
    fn() => json_encode([], '0', 2),
    fn() => json_encode([], 0, '2'),
    fn() => json_decode(123),
    fn() => json_decode('{}', 1),
    fn() => json_decode('{}', null, '2'),
    fn() => json_decode('{}', null, 2, '0'),
];
foreach ($cases as $index => $case) {
    echo $index, '|';
    try {
        var_dump($case());
    } catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "0|TypeError|json_encode(): Argument #2 ($flags) must be of type int, string given\n",
            "1|TypeError|json_encode(): Argument #3 ($depth) must be of type int, string given\n",
            "2|TypeError|json_decode(): Argument #1 ($json) must be of type string, int given\n",
            "3|TypeError|json_decode(): Argument #2 ($associative) must be of type ?bool, int given\n",
            "4|TypeError|json_decode(): Argument #3 ($depth) must be of type int, string given\n",
            "5|TypeError|json_decode(): Argument #4 ($flags) must be of type int, string given\n",
        )
    );
}

#[test]
fn json_new_parameters_cross_dynamic_first_class_callback_and_unpack_calls() {
    assert_eq!(
        run_php(
            r#"<?php
$json = '{"x":1}';
$dynamic = 'json_decode';
$first = json_decode(...);
$calls = [
    $dynamic($json, null, 2, JSON_OBJECT_AS_ARRAY),
    $first($json, null, 2, JSON_OBJECT_AS_ARRAY),
    call_user_func('json_decode', $json, null, 2, JSON_OBJECT_AS_ARRAY),
    json_decode(...['json' => $json, 'flags' => JSON_OBJECT_AS_ARRAY]),
];
foreach ($calls as $value) {
    echo get_debug_type($value), '|', $value['x'], "\n";
}
$encode = 'json_encode';
$firstEncode = json_encode(...);
foreach ([
    $encode(['x' => 1], 0, 1),
    $firstEncode(['x' => 1], 0, 1),
    call_user_func('json_encode', ['x' => 1], 0, 1),
    json_encode(...['value' => ['x' => 1], 'depth' => 1]),
] as $value) {
    echo $value, '|', json_last_error(), "\n";
}
"#,
        ),
        concat!(
            "array|1\n",
            "array|1\n",
            "array|1\n",
            "array|1\n",
            "{\"x\":1}|0\n",
            "{\"x\":1}|0\n",
            "{\"x\":1}|0\n",
            "{\"x\":1}|0\n",
        )
    );
}

#[test]
fn decode_reports_the_first_failure_and_never_repairs_broken_escapes() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    ['syntax-before-utf8', "x\xFF", JSON_INVALID_UTF8_IGNORE, 512],
    ['syntax-before-close', '[x}', 0, 512],
    ['mismatch-before-control', "[}\0", 0, 512],
    ['property-value-syntax', '{"\\u0000x": invalid}', 0, 512],
    ['property-value-depth', '{"\\u0000x":[[]]}', 0, 2],
    ['broken-escape-ignore', "\"a\\\xFFb\"", JSON_INVALID_UTF8_IGNORE, 512],
    ['broken-unicode-substitute', "\"\\u1\xFF234\"", JSON_INVALID_UTF8_SUBSTITUTE, 512],
    ['complete-before-utf8', "1\xFF", JSON_INVALID_UTF8_IGNORE, 512],
];
foreach ($cases as [$label, $json, $flags, $depth]) {
    $value = json_decode($json, null, $depth, $flags);
    echo $label, '|', get_debug_type($value), '|', json_last_error(), "\n";
}
"#,
        ),
        concat!(
            "syntax-before-utf8|null|4\n",
            "syntax-before-close|null|4\n",
            "mismatch-before-control|null|2\n",
            "property-value-syntax|null|4\n",
            "property-value-depth|null|1\n",
            "broken-escape-ignore|null|4\n",
            "broken-unicode-substitute|null|4\n",
            "complete-before-utf8|null|5\n",
        )
    );
}

#[test]
fn decode_invalid_utf8_first_error_matrix_matches_php_85_in_linear_time() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    ['reject-syntax', "x\xFF", null, 512, 0],
    ['keyword', "tru\xFF", null, 512, JSON_INVALID_UTF8_IGNORE],
    ['number', "1e\xFF", null, 512, JSON_INVALID_UTF8_SUBSTITUTE],
    ['unicode-prefix', 'руссиш', null, 512, 0],
    ['unicode-column', '["é"x}', null, 512, 0],
    ['colon-context', '{"a"]', null, 512, 0],
    ['unicode-control', "\"\\u12\x01\"", null, 512, 0],
    ['property', "{\"\\u0000x\":1}\xFF", null, 512, JSON_INVALID_UTF8_IGNORE],
    ['property-array', "{\"\\u0000x\":1}\xFF", true, 512, JSON_INVALID_UTF8_IGNORE],
    ['depth', "[[0]]\xFF", true, 2, JSON_INVALID_UTF8_IGNORE],
    ['utf16', "\"\\uD800\"\xFF", null, 512, JSON_INVALID_UTF8_IGNORE],
    ['utf16-open', "\"\\uD800\xFF\"", null, 512, JSON_INVALID_UTF8_IGNORE],
    ['utf16-escape', "\"\\uD800\\\xFF\"", null, 512, JSON_INVALID_UTF8_IGNORE],
    ['utf16-low', "\"\\uD800\\uDC\xFF\"", null, 512, JSON_INVALID_UTF8_IGNORE],
    ['overflow', "1e400\xFF", null, 512, JSON_INVALID_UTF8_IGNORE],
    ['bigint', "9223372036854775808\xFF", null, 512,
        JSON_BIGINT_AS_STRING | JSON_INVALID_UTF8_IGNORE],
    ['incomplete-container', "[\xFF]", null, 512, JSON_INVALID_UTF8_IGNORE],
];
foreach ($cases as [$label, $json, $associative, $depth, $flags]) {
    $value = json_decode($json, $associative, $depth, $flags);
    echo $label, '|', get_debug_type($value), '|', json_last_error(), "\n";
}
$many = '"' . str_repeat("\xFF", 4096) . '"';
foreach ([JSON_INVALID_UTF8_IGNORE, JSON_INVALID_UTF8_SUBSTITUTE] as $flags) {
    $value = json_decode($many, null, 512, $flags);
    echo 'many-', $flags, '|', strlen($value), '|', json_last_error(), "\n";
}
"#,
        ),
        concat!(
            "reject-syntax|null|4\n",
            "keyword|null|4\n",
            "number|null|4\n",
            "unicode-prefix|null|4\n",
            "unicode-column|null|4\n",
            "colon-context|null|4\n",
            "unicode-control|null|4\n",
            "property|null|9\n",
            "property-array|null|5\n",
            "depth|null|1\n",
            "utf16|null|10\n",
            "utf16-open|null|10\n",
            "utf16-escape|null|10\n",
            "utf16-low|null|10\n",
            "overflow|null|5\n",
            "bigint|null|5\n",
            "incomplete-container|null|5\n",
            "many-1048576|0|0\n",
            "many-2097152|12288|0\n",
        )
    );
}

#[test]
fn decode_depth_validation_resets_only_the_non_throwing_error_channel() {
    assert_eq!(
        run_php(
            r#"<?php
json_decode('{');
try { json_decode('[]', null, 0); } catch (Throwable $error) {
    echo 'plain|', get_class($error), '|', json_last_error(), "\n";
}
json_decode('{');
try { json_decode('[]', null, 0, JSON_THROW_ON_ERROR); } catch (Throwable $error) {
    echo 'throw|', get_class($error), '|', json_last_error(), "\n";
}
"#,
        ),
        "plain|ValueError|0\nthrow|ValueError|4\n"
    );
}

#[test]
fn encode_resources_closures_and_multiple_errors_follow_php_traversal() {
    assert_eq!(
        run_php(
            r#"<?php
$resource = fopen('php://memory', 'r+');
foreach ([0, JSON_PARTIAL_OUTPUT_ON_ERROR] as $flags) {
    $result = json_encode($resource, $flags);
    echo 'resource-', $flags, '|', get_debug_type($result), '|',
        $result === false ? '-' : $result, '|', json_last_error(), "\n";
}
json_decode('{');
try { json_encode($resource, JSON_THROW_ON_ERROR); } catch (Throwable $error) {
    echo 'resource-throw|', $error->getCode(), '|', json_last_error(), "\n";
}
echo 'closure|', json_encode(static fn() => 1), '|', json_last_error(), "\n";

$cases = [
    ['utf8-nan', ["\xFF", NAN]],
    ['depth-utf8', [[[1]], "\xFF"]],
    ['resource-nan', [$resource, NAN]],
    ['nan-resource', [NAN, $resource]],
];
foreach ($cases as [$label, $value]) {
    foreach ([0, JSON_PARTIAL_OUTPUT_ON_ERROR] as $flags) {
        $result = json_encode($value, $flags, 1);
        echo $label, '-', $flags, '|', get_debug_type($result), '|',
            $result === false ? '-' : $result, '|', json_last_error(), "\n";
    }
}
"#,
        ),
        concat!(
            "resource-0|bool|-|8\n",
            "resource-512|string|null|8\n",
            "resource-throw|8|4\n",
            "closure|{}|0\n",
            "utf8-nan-0|bool|-|5\n",
            "utf8-nan-512|string|[null,0]|7\n",
            "depth-utf8-0|bool|-|1\n",
            "depth-utf8-512|string|[[[1]],null]|5\n",
            "resource-nan-0|bool|-|8\n",
            "resource-nan-512|string|[null,0]|7\n",
            "nan-resource-0|bool|-|8\n",
            "nan-resource-512|string|[0,null]|8\n",
        )
    );
}

#[test]
fn encode_depth_uses_php_int_truncation_and_weak_integer_strings_stay_exact() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([2147483648, 4294967297, 4294967298] as $depth) {
    $result = json_encode([[]], 0, $depth);
    echo $depth, '|', get_debug_type($result), '|',
        $result === false ? '-' : $result, '|', json_last_error(), "\n";
}
try {
    json_decode('[]', null, '9223372036854775807');
} catch (Throwable $error) {
    echo 'max-string|', get_class($error), '|', $error->getMessage(), "\n";
}
$value = json_decode('{"x":1}', null, 2, '9223372036854775807');
echo 'flags-string|', get_debug_type($value), '|', $value['x'], "\n";
"#,
        ),
        concat!(
            "2147483648|bool|-|1\n",
            "4294967297|bool|-|1\n",
            "4294967298|string|[[]]|0\n",
            "max-string|ValueError|json_decode(): Argument #3 ($depth) must be less than 2147483647\n",
            "flags-string|array|1\n",
        )
    );
}

#[test]
fn decode_deep_parser_boundary_is_safe_and_matches_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([4096, 4998, 4999] as $depth) {
    $json = str_repeat('[', $depth) . '0' . str_repeat(']', $depth);
    $value = json_decode($json, true, $depth + 1);
    echo 'a', $depth, '|', get_debug_type($value), '|', json_last_error(), "\n";
}
foreach ([2499, 2500] as $depth) {
    $json = str_repeat('{"x":', $depth) . '0' . str_repeat('}', $depth);
    $value = json_decode($json, false, $depth + 1);
    echo 'o', $depth, '|', get_debug_type($value), '|', json_last_error(), "\n";
}
foreach ([[2498, 1250, 3749], [2499, 1250, 3750], [2499, 1250, 3749]] as [$arrays, $objects, $depth]) {
    $json = str_repeat('[', $arrays)
        . str_repeat('{"x":', $objects)
        . '0'
        . str_repeat('}', $objects)
        . str_repeat(']', $arrays);
    $value = json_decode($json, false, $depth);
    echo 'm', $arrays, '/', $objects, '/', $depth, '|',
        get_debug_type($value), '|', json_last_error(), "\n";
}
$malformed = str_repeat('[', 4096) . '0' . str_repeat(']', 4095);
$value = json_decode($malformed, true, 5000);
echo 'malformed|', get_debug_type($value), '|', json_last_error(), "\n";
"#,
        ),
        concat!(
            "a4096|array|0\n",
            "a4998|array|0\n",
            "a4999|null|4\n",
            "o2499|stdClass|0\n",
            "o2500|null|4\n",
            "m2498/1250/3749|array|0\n",
            "m2499/1250/3750|null|4\n",
            "m2499/1250/3749|null|1\n",
            "malformed|null|4\n",
        )
    );
}

#[test]
fn deep_json_abort_and_release_paths_are_stack_safe_and_keep_drop_order() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 0;
for ($index = 0; $index < 6000; $index++) { $value = [$value]; }
$result = json_encode($value, 0, 1);
echo 'array-depth|', get_debug_type($result), '|', json_last_error(), '|';
unset($value, $result);
echo "released\n";

$value = (object) [];
for ($index = 0; $index < 6000; $index++) { $value = (object) ['x' => $value]; }
$result = json_encode($value, 0, 200);
echo 'object-depth|', get_debug_type($result), '|', json_last_error(), '|';
unset($value, $result);
echo "released\n";

class DeepJsonTemporary implements JsonSerializable {
    public function jsonSerialize(): mixed {
        $value = 0;
        for ($index = 0; $index < 6000; $index++) { $value = [$value]; }
        return $value;
    }
}
$result = json_encode(new DeepJsonTemporary, 0, 1);
echo 'temporary|', get_debug_type($result), '|', json_last_error(), "|released\n";

$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$value = ["\xFF", $deep];
unset($deep);
$result = json_encode($value, 0, 10000);
echo 'early-utf8|', get_debug_type($result), '|', json_last_error(), '|';
unset($value, $result);
echo "released\n";

$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$value = [$deep, "\xFF"];
unset($deep);
$result = json_encode($value, 0, 10000);
echo 'late-utf8|', get_debug_type($result), '|', json_last_error(), '|';
unset($value, $result);
echo "released\n";

$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$value = (object) ['a' => $deep, 'z' => "\xFF"];
unset($deep);
$result = json_encode($value, 0, 10000);
echo 'late-object-utf8|', get_debug_type($result), '|', json_last_error(), '|';
unset($value, $result);
echo "released\n";

$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$resource = fopen('php://memory', 'r');
$value = [$deep, $resource];
unset($deep);
$result = json_encode($value, 0, 10000);
echo 'late-resource|', get_debug_type($result), '|', json_last_error(), '|';
unset($value, $result);
fclose($resource);
echo "released\n";

class JsonDeepLateThrow implements JsonSerializable {
    public function jsonSerialize(): mixed { throw new Exception('late-stop'); }
}
$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$value = [$deep, new JsonDeepLateThrow];
unset($deep);
echo 'late-callback|';
try { json_encode($value, 0, 10000); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), '|';
}
unset($value);
echo "released\n";

$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$value = ["\xFF", &$deep];
unset($deep);
json_encode($value, 0, 10000);
unset($value);
echo "early-reference|released\n";

$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$closure = function () use ($deep): void {};
unset($deep);
$value = ["\xFF", $closure];
unset($closure);
json_encode($value, 0, 10000);
unset($value);
echo "early-closure|released\n";

function discardedDeepClosure(): Closure {
    $deep = 0;
    for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
    return function () use ($deep): void {};
}
discardedDeepClosure();
echo "discarded-closure|released\n";

function deepJsonGenerator(mixed $value): Generator { yield 1; }
$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$generator = deepJsonGenerator($deep);
unset($deep);
$value = ["\xFF", $generator];
unset($generator);
json_encode($value, 0, 10000);
unset($value);
echo "early-generator|released\n";

$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$root = ["\xFF", $deep];
unset($deep);
json_encode($root, 0, 10000);
$escaped = $root[1];
$copy = array_values($root);
unset($root, $escaped, $copy);
echo "escaped-array|released\n";

$root = ["\xFF"];
json_encode($root);
$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$root[] = $deep;
unset($deep);
json_encode($root, 0, 10000);
$escaped = $root[1];
unset($root, $escaped);
echo "mutated-array|released\n";

$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = (object) ['x' => $deep]; }
$root = (object) ['a' => "\xFF", 'b' => $deep];
unset($deep);
json_encode($root, 0, 10000);
$escaped = $root->b;
$copy = clone $root;
unset($root, $escaped, $copy);
echo "escaped-object|released\n";

$root = (object) ['a' => "\xFF"];
json_encode($root);
$copy = clone $root;
$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$copy->b = $deep;
unset($deep, $root, $copy);
echo "mutated-clone|released\n";

foreach ([63, 64, 65, 1000] as $depth) {
    $cycle = [];
    $cycle[] =& $cycle;
    $value = $cycle;
    for ($index = 0; $index < $depth; $index++) { $value = [$value]; }
    $result = json_encode($value, 0, 2000);
    echo 'recursion-', $depth, '|', get_debug_type($result), '|', json_last_error(), "\n";
    unset($cycle, $value, $result);
}
$shared = ['x' => 1];
$value = [$shared, $shared];
for ($index = 0; $index < 64; $index++) { $value = [$value]; }
$result = json_encode($value, 0, 2000);
echo 'deep-siblings|', get_debug_type($result), '|', strlen($result), '|', json_last_error(), "\n";
unset($shared, $value, $result);

$first = new stdClass;
$second = new stdClass;
$firstId = spl_object_id($first);
$secondId = spl_object_id($second);
$value = [$first, $second];
unset($first, $second);
for ($index = 0; $index < 300; $index++) { $value = [$value]; }
json_encode($value, 0, 1000);
unset($value);
$newId = spl_object_id(new stdClass);
echo 'drop-order|', $newId === $secondId ? 1 : 0, "\n";

class JsonThrowsFirst implements JsonSerializable {
    public function jsonSerialize(): mixed { echo 'first'; throw new Exception('stop'); }
}
class JsonMustNotRun implements JsonSerializable {
    public function jsonSerialize(): mixed { echo 'later'; return 1; }
}
$value = [new JsonThrowsFirst, new JsonMustNotRun];
for ($index = 0; $index < 300; $index++) { $value = [$value]; }
echo 'callback-order|';
try { json_encode($value, 0, 1000); } catch (Throwable $error) {
    echo '|', get_class($error), '|';
}
unset($value);
echo "released\n";

class DeepDestructor {
    public mixed $x;
    public function __destruct() { echo 'D'; }
}
$value = new DeepDestructor;
for ($index = 0; $index < 5000; $index++) { $value = [$value]; }
unset($value);
echo '|';
$value = new DeepDestructor;
for ($index = 0; $index < 5000; $index++) { $value = (object) ['x' => $value]; }
unset($value);
echo "|done\n";

$holder = new DeepDestructor;
$deep = 0;
for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
$holder->x = $deep;
unset($deep, $holder);
echo "|holder\n";

function unwindDeepJsonSlots(): void {
    $first = new DeepDestructor;
    $deep = 0;
    for ($index = 0; $index < 6000; $index++) { $deep = [$deep]; }
    $closure = function () use ($deep): void {};
    unset($deep);
    throw new Exception('stop');
}
try {
    unwindDeepJsonSlots();
} catch (Throwable $error) {
    echo '|caught', "\n";
}

class DeepSilentDestructor { public function __destruct() {} }
gc_collect_cycles();
$beforeRoots = gc_status()['roots'];
$value = new DeepSilentDestructor;
for ($index = 0; $index < 1000; $index++) { $value = [$value]; }
unset($value);
$afterRoots = gc_status()['roots'];
echo 'gc-roots|', $afterRoots - $beforeRoots, '|', gc_collect_cycles(), "\n";

$target = new stdClass;
$weak = WeakReference::create($target);
$value = $target;
unset($target);
for ($index = 0; $index < 5000; $index++) { $value = (object) ['x' => $value]; }
unset($value);
echo 'weak|', $weak->get() === null ? 'null' : 'live', "\n";

class WeakClosureDestructor {
    public function __destruct() { echo 'W|'; }
}
$object = new WeakClosureDestructor;
$closure = function () use ($object): void {};
unset($object);
$weak = WeakReference::create($closure);
unset($closure);
echo 'weak-closure|', $weak->get() === null ? 'null' : 'live', "\n";

class DeepCycleDestructor {
    public mixed $x;
    public function __destruct() { echo 'C|'; }
}
$cycleOwner = new DeepCycleDestructor;
$cycleChain = $cycleOwner;
for ($index = 0; $index < 300; $index++) { $cycleChain = [$cycleChain]; }
$cycleOwner->x = $cycleChain;
unset($cycleChain, $cycleOwner);
echo 'cycle-gc|', gc_collect_cycles(), "\n";
"#,
        ),
        concat!(
            "array-depth|bool|1|released\n",
            "object-depth|bool|1|released\n",
            "temporary|bool|1|released\n",
            "early-utf8|bool|5|released\n",
            "late-utf8|bool|5|released\n",
            "late-object-utf8|bool|5|released\n",
            "late-resource|bool|8|released\n",
            "late-callback|Exception|late-stop|released\n",
            "early-reference|released\n",
            "early-closure|released\n",
            "discarded-closure|released\n",
            "early-generator|released\n",
            "escaped-array|released\n",
            "mutated-array|released\n",
            "escaped-object|released\n",
            "mutated-clone|released\n",
            "recursion-63|bool|6\n",
            "recursion-64|bool|6\n",
            "recursion-65|bool|6\n",
            "recursion-1000|bool|6\n",
            "deep-siblings|string|145|0\n",
            "drop-order|1\n",
            "callback-order|first|Exception|released\n",
            "D|D|done\n",
            "D|holder\n",
            "D|caught\n",
            "gc-roots|0|0\n",
            "weak|null\n",
            "W|weak-closure|null\n",
            "cycle-gc|C|301\n",
        )
    );
}

#[test]
fn decoded_deep_shared_subtrees_survive_cow_and_release_stack_safely() {
    assert_eq!(
        run_php(
            r#"<?php
$json = str_repeat('[', 1000) . '0' . str_repeat(']', 1000);
$root = json_decode($json, true, 1001);
$subtree = $root[0];
$copy = $root;
$copy[0] = 'changed';
unset($root, $copy);
$depth = 0;
while (is_array($subtree)) { $subtree = $subtree[0]; $depth++; }
echo 'array|', $depth, '|', $subtree, '|', json_last_error(), "\n";
unset($subtree);

$json = str_repeat('{"x":', 1000) . '0' . str_repeat('}', 1000);
$root = json_decode($json, false, 1001);
$subtree = $root->x;
unset($root);
$depth = 0;
while (is_object($subtree)) { $subtree = $subtree->x; $depth++; }
echo 'object|', $depth, '|', $subtree, '|', json_last_error(), "\n";
unset($subtree);
"#,
        ),
        "array|999|0|0\nobject|999|0|0\n"
    );
}

#[test]
fn closure_drop_during_captured_object_property_replacement_is_borrow_safe() {
    assert_eq!(
        run_php(
            r#"<?php
$object = new stdClass;
$object->callback = function () use ($object): void {};
$object->callback = null;
echo "released\n";
"#,
        ),
        "released\n"
    );
}

#[test]
fn encode_deep_container_boundary_is_stack_safe_and_matches_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$value = (object) [];
for ($index = 0; $index < 20000; $index++) {
    $value = (object) ['x' => $value];
}
$result = json_encode($value, 0, 500000);
echo 'o20000|', get_debug_type($result), '|', strlen($result), '|', json_last_error(), "\n";
unset($value, $result);

$value = (object) [];
for ($index = 0; $index < 25000; $index++) {
    $value = (object) ['x' => $value];
}
$result = json_encode($value, 0, 500000);
echo 'o25000|', get_debug_type($result), '|', json_last_error(), "\n";
$result = json_encode($value, JSON_PARTIAL_OUTPUT_ON_ERROR, 500000);
echo 'o25000-partial|', get_debug_type($result), '|',
    strlen($result) > 120000 && strlen($result) < 150000 ? 'bounded' : 'bad', '|',
    json_last_error(), "\n";
"#,
        ),
        concat!(
            "o20000|string|120002|0\n",
            "o25000|bool|1\n",
            "o25000-partial|string|bounded|1\n",
        )
    );
}
