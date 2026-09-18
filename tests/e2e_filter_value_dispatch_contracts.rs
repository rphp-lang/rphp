mod common;

use common::run_php;

#[test]
fn integer_filter_accepts_php_radices_and_checked_amd64_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    [' 234 ', 0],
    ['0Xff', FILTER_FLAG_ALLOW_HEX],
    ['-0xff', FILTER_FLAG_ALLOW_HEX],
    ['0666', FILTER_FLAG_ALLOW_OCTAL],
    ['0o16', FILTER_FLAG_ALLOW_OCTAL],
    ['0xffffffffffffffff', FILTER_FLAG_ALLOW_HEX],
    ['01777777777777777777777', FILTER_FLAG_ALLOW_OCTAL],
    ['9223372036854775808', 0],
] as [$value, $flags]) {
    var_dump(filter_var($value, FILTER_VALIDATE_INT, $flags));
}
"#,
        ),
        concat!(
            "int(234)\n",
            "int(255)\n",
            "bool(false)\n",
            "int(438)\n",
            "int(14)\n",
            "int(-1)\n",
            "int(-1)\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn float_filter_trims_input_honors_decimal_and_rejects_non_finite_results() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(filter_var('  7E-3  ', FILTER_VALIDATE_FLOAT));
var_dump(filter_var('1,234', FILTER_VALIDATE_FLOAT, [
    'options' => ['decimal' => ','],
]));
var_dump(filter_var('1.234', FILTER_VALIDATE_FLOAT, [
    'options' => ['decimal' => ','],
]));
var_dump(filter_var('1e+309', FILTER_VALIDATE_FLOAT));
var_dump(filter_var('1e-324', FILTER_VALIDATE_FLOAT));
try {
    filter_var('1', FILTER_VALIDATE_FLOAT, ['options' => ['decimal' => '..']]);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "float(0.007)\n",
            "float(1.234)\n",
            "bool(false)\n",
            "bool(false)\n",
            "bool(false)\n",
            "ValueError:filter_var(): \"decimal\" option must be one character long\n",
        )
    );
}

#[test]
fn array_shape_flags_filter_nested_values_without_mutating_the_source() {
    assert_eq!(
        run_php(
            r#"<?php
$source = [1, '1', '', -1, ['yes', 'none']];
$filtered = filter_var($source, FILTER_VALIDATE_BOOL, FILTER_REQUIRE_ARRAY);
$filtered[4][0] = false;
var_dump($filtered, $source);
var_dump(filter_var('12', FILTER_VALIDATE_INT, FILTER_FORCE_ARRAY));
var_dump(filter_var($source, FILTER_VALIDATE_INT, FILTER_REQUIRE_SCALAR));
"#,
        ),
        concat!(
            "array(5) {\n",
            "  [0]=>\n  bool(true)\n",
            "  [1]=>\n  bool(true)\n",
            "  [2]=>\n  bool(false)\n",
            "  [3]=>\n  bool(false)\n",
            "  [4]=>\n  array(2) {\n",
            "    [0]=>\n    bool(false)\n",
            "    [1]=>\n    bool(false)\n",
            "  }\n}\n",
            "array(5) {\n",
            "  [0]=>\n  int(1)\n",
            "  [1]=>\n  string(1) \"1\"\n",
            "  [2]=>\n  string(0) \"\"\n",
            "  [3]=>\n  int(-1)\n",
            "  [4]=>\n  array(2) {\n",
            "    [0]=>\n    string(3) \"yes\"\n",
            "    [1]=>\n    string(4) \"none\"\n",
            "  }\n}\n",
            "array(1) {\n  [0]=>\n  int(12)\n}\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn callback_filter_uses_canonical_call_frames_and_recursive_order() {
    assert_eq!(
        run_php(
            r#"<?php
$calls = [];
$callback = function ($value) use (&$calls) {
    $calls[] = $value;
    return strtoupper($value);
};
var_dump(filter_var('ab', FILTER_CALLBACK, ['options' => $callback]));
var_dump(filter_var(['ab', ['cd']], FILTER_CALLBACK, [
    'options' => $callback,
    'flags' => FILTER_REQUIRE_ARRAY,
]));
var_dump($calls);
set_error_handler(function ($level, $message) { echo "warning:$message\n"; });
function wants_reference(&$value) { $value = 'changed'; }
var_dump(filter_var('kept', FILTER_CALLBACK, ['options' => 'wants_reference']));
restore_error_handler();
"#,
        ),
        concat!(
            "string(2) \"AB\"\n",
            "array(2) {\n",
            "  [0]=>\n  string(2) \"AB\"\n",
            "  [1]=>\n  array(1) {\n",
            "    [0]=>\n    string(2) \"CD\"\n",
            "  }\n}\n",
            "array(3) {\n",
            "  [0]=>\n  string(2) \"ab\"\n",
            "  [1]=>\n  string(2) \"ab\"\n",
            "  [2]=>\n  string(2) \"cd\"\n",
            "}\n",
            "warning:wants_reference(): Argument #1 ($value) must be passed by reference, value given\n",
            "NULL\n",
        )
    );
}

#[test]
fn rejected_shapes_precede_callback_validation_but_scalar_calls_validate_it() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(filter_var([], FILTER_CALLBACK));
var_dump(filter_var([], FILTER_CALLBACK, FILTER_REQUIRE_ARRAY));
var_dump(filter_var('x', FILTER_CALLBACK, FILTER_REQUIRE_ARRAY));
try { filter_var('x', FILTER_CALLBACK); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "bool(false)\n",
            "array(0) {\n}\n",
            "bool(false)\n",
            "TypeError:filter_var(): Option must be a valid callback\n",
        )
    );
}
