mod common;

use common::run_php;

#[test]
fn json_validate_exposes_the_php_85_reflection_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$function = new ReflectionFunction('json_validate');
echo $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), '|',
    $function->getReturnType(), '|', $function->getExtensionName(), "\n";
foreach ($function->getParameters() as $parameter) {
    echo $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isOptional() ? 'optional' : 'required', ':',
        $parameter->isDefaultValueAvailable()
            ? serialize($parameter->getDefaultValue())
            : '-', "\n";
}
"#,
        ),
        concat!(
            "1/3|bool|json\n",
            "json:string:required:-\n",
            "depth:int:optional:i:512;\n",
            "flags:int:optional:i:0;\n",
        )
    );
}

#[test]
fn json_validate_accepts_scalars_strings_and_nested_containers() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'null', 'true', '-42', '6.25e-3', '"a\n\uD83D\uDE00"',
    '[1,{"x":false}]', '{"left":{"items":[1,2]},"ok":true}',
] as $json) {
    echo json_validate($json) ? 'T' : 'F', ':', json_last_error(), '|';
}
"#,
        ),
        "T:0|T:0|T:0|T:0|T:0|T:0|T:0|"
    );
}

#[test]
fn json_validate_reports_php_json_error_categories() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    '' => 4,
    '[}' => 2,
    "\"a" . chr(1) . "b\"" => 3,
    '"\uD800"' => 10,
    '[1,]' => 4,
    "\xEF\xBB\xBF{}" => 4,
] as $json => $expected) {
    echo json_validate($json) ? 'T' : 'F', ':', json_last_error(), ':',
        json_last_error() === $expected ? 'same' : 'different', '|';
}
"#,
        ),
        "F:4:same|F:2:same|F:3:same|F:10:same|F:4:same|F:4:same|"
    );
}

#[test]
fn json_validate_enforces_container_depth_without_rejecting_scalars() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    ['1', 1], ['[]', 1], ['[]', 2], ['[[]]', 2], ['[[]]', 3],
    ['{}', 1], ['{}', 2], ['{"x":{}}', 2], ['{"x":{}}', 3],
] as [$json, $depth]) {
    echo json_validate($json, $depth) ? 'T' : 'F', ':', json_last_error(), '|';
}
"#,
        ),
        "T:0|F:1|T:0|F:1|T:0|F:1|T:0|F:1|T:0|"
    );
}

#[test]
fn json_validate_preserves_empty_input_depth_precedence_and_range_errors() {
    assert_eq!(
        run_php(
            r#"<?php
function attempt(string $label, string $json, int $depth): void {
    json_decode('{');
    try {
        $result = json_validate($json, $depth);
        echo $label, ':return:', $result ? 'T' : 'F', ':', json_last_error(), '|';
    } catch (Throwable $error) {
        echo $label, ':', get_class($error), ':', $error->getMessage(), ':',
            json_last_error(), '|';
    }
}
attempt('empty', '', 0);
attempt('zero', '1', 0);
attempt('maximum', '1', 2147483647);
attempt('too-large', '1', PHP_INT_MAX);
"#,
        ),
        concat!(
            "empty:return:F:4|",
            "zero:ValueError:json_validate(): Argument #2 ($depth) must be greater than 0:0|",
            "maximum:return:T:0|",
            "too-large:ValueError:json_validate(): Argument #2 ($depth) must be less than 2147483647:0|",
        )
    );
}

#[test]
fn json_validate_accepts_only_the_invalid_utf8_ignore_flag() {
    assert_eq!(
        run_php(
            r#"<?php
json_decode('{');
try {
    json_validate('{}', 512, JSON_BIGINT_AS_STRING);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), ':', json_last_error(), '|';
}
try {
    json_validate('{}', 0, JSON_INVALID_UTF8_IGNORE | JSON_BIGINT_AS_STRING);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), ':', json_last_error(), '|';
}
echo json_validate('{}', flags: JSON_INVALID_UTF8_IGNORE) ? 'T' : 'F', ':',
    json_last_error();
"#,
        ),
        concat!(
            "ValueError:json_validate(): Argument #3 ($flags) must be a valid flag ",
            "(allowed flags: JSON_INVALID_UTF8_IGNORE):4|",
            "ValueError:json_validate(): Argument #3 ($flags) must be a valid flag ",
            "(allowed flags: JSON_INVALID_UTF8_IGNORE):4|T:0",
        )
    );
}

#[test]
fn json_validate_rejects_or_ignores_invalid_utf8_inside_strings() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (["\"a" . chr(255) . "b\"", "[\"" . chr(193) . chr(193) . "\",1]"] as $json) {
    echo json_validate($json) ? 'T' : 'F', ':', json_last_error(), '|';
    echo json_validate($json, flags: JSON_INVALID_UTF8_IGNORE) ? 'T' : 'F', ':',
        json_last_error(), '|';
}
"#,
        ),
        "F:5|T:0|F:5|T:0|"
    );
}

#[test]
fn json_validate_does_not_ignore_structural_or_escape_breaking_bytes() {
    assert_eq!(
        run_php(
            r#"<?php
$inputs = [
    "\"a\\" . chr(255) . "\"",
    "\"\\uD8" . chr(255) . "\"",
    '[' . chr(255) . ']',
    '{} ' . chr(255),
    '[}' . chr(255),
];
foreach ($inputs as $json) {
    echo json_validate($json, flags: JSON_INVALID_UTF8_IGNORE) ? 'T' : 'F', ':',
        json_last_error(), '|';
}
"#,
        ),
        "F:4|F:4|F:5|F:5|F:2|"
    );
}

#[test]
fn json_validate_accepts_duplicate_nul_keys_and_numeric_overflow() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    '{"a":1,"a":2}',
    '{"\u0000hidden":1}',
    '1e400',
    '-1e400',
    '999999999999999999999999999999999999999999999999',
] as $json) {
    echo json_validate($json) ? 'T' : 'F', ':', json_last_error(), '|';
}
"#,
        ),
        "T:0|T:0|T:0|T:0|T:0|"
    );
}

#[test]
fn json_validate_uses_weak_string_coercion_and_stringable_objects() {
    assert_eq!(
        run_php(
            r#"<?php
class JsonText {
    public function __toString(): string {
        echo 'converted|';
        return '{}';
    }
}
foreach ([false, true, 12, 1.5, new JsonText(), [1]] as $value) {
    try {
        echo json_validate($value) ? 'T' : 'F', ':', json_last_error(), '|';
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), '|';
    }
}
"#,
        ),
        concat!(
            "F:4|T:0|T:0|T:0|converted|T:0|",
            "TypeError:json_validate(): Argument #1 ($json) must be of type string, array given|",
        )
    );
}

#[test]
fn json_validate_honors_strict_scalar_parameter_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([false, 12, 1.5] as $value) {
    try {
        json_validate($value);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), '|';
    }
}
"#,
        ),
        concat!(
            "TypeError:json_validate(): Argument #1 ($json) must be of type string, false given|",
            "TypeError:json_validate(): Argument #1 ($json) must be of type string, int given|",
            "TypeError:json_validate(): Argument #1 ($json) must be of type string, float given|",
        )
    );
}

#[test]
fn json_validate_supports_named_unpacked_dynamic_and_first_class_calls() {
    assert_eq!(
        run_php(
            r#"<?php
$callable = json_validate(...);
foreach ([
    ['json' => '{}'],
    ['flags' => 0, 'json' => '[1]'],
    ['depth' => 2, 'json' => '[[]]'],
    ['json' => '{', 'flags' => 0],
] as $arguments) {
    echo $callable(...$arguments) ? 'T' : 'F', ':', json_last_error(), '|';
}
echo call_user_func('json_validate', 'true') ? 'T' : 'F', ':', json_last_error();
"#,
        ),
        "T:0|T:0|F:1|F:4|T:0"
    );
}

#[test]
fn json_validate_updates_and_resets_the_request_local_error_channel() {
    assert_eq!(
        run_php(
            r#"<?php
json_decode('{');
echo json_last_error(), '|';
try {
    json_validate('{}', flags: JSON_BIGINT_AS_STRING);
} catch (Throwable) {
    echo json_last_error(), '|';
}
try {
    json_validate('1', 0);
} catch (Throwable) {
    echo json_last_error(), '|';
}
echo json_validate('{') ? 'T' : 'F', ':', json_last_error(), '|';
echo json_validate('{}') ? 'T' : 'F', ':', json_last_error(), '|';
echo json_validate('') ? 'T' : 'F', ':', json_last_error();
"#,
        ),
        "4|4|0|F:4|T:0|F:4"
    );
}

#[test]
fn json_validate_rejects_trailing_tokens_and_partial_scalar_lexemes() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['{}x', 'true false', 'tru', '01', '1e', '-', '"unterminated'] as $json) {
    echo json_validate($json) ? 'T' : 'F', ':', json_last_error(), '|';
}
"#,
        ),
        "F:4|F:4|F:4|F:4|F:4|F:4|F:4|"
    );
}

#[test]
fn json_validate_enforces_arity_and_named_parameter_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    static fn() => json_validate(),
    static fn() => json_validate('{}', 512, 0, 1),
    static fn() => json_validate(json: '{}', unknown: 1),
] as $callable) {
    try {
        $callable();
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), '|';
    }
}
"#,
        ),
        concat!(
            "ArgumentCountError:json_validate() expects at least 1 argument, 0 given|",
            "ArgumentCountError:json_validate() expects at most 3 arguments, 4 given|",
            "Error:Unknown named parameter $unknown|",
        )
    );
}

#[test]
fn json_validate_coerces_or_rejects_depth_and_flags_in_weak_calls() {
    assert_eq!(
        run_php(
            r#"<?php
echo json_validate('[]', '2', '0') ? 'T' : 'F', ':', json_last_error(), '|';
foreach ([
    static fn() => json_validate('{}', []),
    static fn() => json_validate('{}', 512, []),
] as $callable) {
    try {
        $callable();
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), '|';
    }
}
"#,
        ),
        concat!(
            "T:0|",
            "TypeError:json_validate(): Argument #2 ($depth) must be of type int, array given|",
            "TypeError:json_validate(): Argument #3 ($flags) must be of type int, array given|",
        )
    );
}

#[test]
fn json_validate_keeps_strict_depth_and_flag_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([
    static fn() => json_validate('{}', '2'),
    static fn() => json_validate('{}', 512, '0'),
] as $callable) {
    try {
        $callable();
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), '|';
    }
}
"#,
        ),
        concat!(
            "TypeError:json_validate(): Argument #2 ($depth) must be of type int, string given|",
            "TypeError:json_validate(): Argument #3 ($flags) must be of type int, string given|",
        )
    );
}

#[test]
fn json_validate_matches_the_php_parser_stack_ceiling_without_native_overflow() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([4998, 4999] as $depth) {
    $json = str_repeat('[', $depth) . '0' . str_repeat(']', $depth);
    echo json_validate($json, 2147483647) ? 'T' : 'F', ':', json_last_error(), '|';
}
foreach ([2499, 2500] as $depth) {
    $json = str_repeat('{"x":', $depth) . '0' . str_repeat('}', $depth);
    echo json_validate($json, 2147483647) ? 'T' : 'F', ':', json_last_error(), '|';
}
"#,
        ),
        "T:0|F:4|T:0|F:4|"
    );
}
