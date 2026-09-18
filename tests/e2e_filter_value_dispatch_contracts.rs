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

#[test]
fn filter_inventory_and_deprecated_aliases_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(count(filter_list()), filter_id('validate_email'), filter_id('missing'));
set_error_handler(function ($level, $message) { echo $level, ':', $message, "\n"; });
var_dump(filter_var('<b>x</b>', FILTER_SANITIZE_STRING));
restore_error_handler();
"#,
        ),
        concat!(
            "int(21)\n",
            "int(274)\n",
            "bool(false)\n",
            "8192:Constant FILTER_SANITIZE_STRING is deprecated since 8.1, use htmlspecialchars() instead\n",
            "string(1) \"x\"\n",
        )
    );
}

#[test]
fn sanitizer_ip_and_mac_edges_use_php_byte_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(filter_var("ки", FILTER_SANITIZE_SPECIAL_CHARS, FILTER_FLAG_ENCODE_HIGH));
var_dump(filter_var('0123.4567.89ab', FILTER_VALIDATE_MAC));
var_dump(filter_var('224.0.0.0', FILTER_VALIDATE_IP, FILTER_FLAG_NO_RES_RANGE));
var_dump(filter_var('240.0.0.0', FILTER_VALIDATE_IP, FILTER_FLAG_NO_RES_RANGE));
"#,
        ),
        concat!(
            "string(24) \"&#208;&#186;&#208;&#184;\"\n",
            "string(14) \"0123.4567.89ab\"\n",
            "string(9) \"224.0.0.0\"\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn filter_var_array_preserves_only_nested_array_reference_cells() {
    assert_eq!(
        run_php(
            r#"<?php
$scalar = '1';
$scalarInput = [&$scalar];
var_dump(filter_var_array($scalarInput, FILTER_VALIDATE_INT), $scalar);
$nested = ['123foo'];
$nestedInput = [&$nested];
var_dump(filter_var_array($nestedInput, FILTER_VALIDATE_INT), $nested);
"#,
        ),
        concat!(
            "array(1) {\n  [0]=>\n  int(1)\n}\n",
            "string(1) \"1\"\n",
            "array(1) {\n  [0]=>\n  &array(1) {\n    [0]=>\n    bool(false)\n  }\n}\n",
            "array(1) {\n  [0]=>\n  bool(false)\n}\n",
        )
    );
}

#[test]
fn filter_throw_on_failure_uses_the_namespaced_exception_hierarchy() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([new stdClass(), 'bad'] as $value) {
    try { filter_var($value, FILTER_VALIDATE_EMAIL, FILTER_THROW_ON_FAILURE); }
    catch (Filter\FilterException $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "Filter\\FilterFailedException:filter validation failed: object of type stdClass has no __toString() method\n",
            "Filter\\FilterFailedException:filter validation failed: filter validate_email not satisfied by 'bad'\n",
        )
    );
}
