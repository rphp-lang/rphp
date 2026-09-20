mod common;

use common::run_php;

#[test]
fn numeric_string_comparison_preserves_decimal_integer_precision() {
    assert_eq!(
        run_php(
            r#"<?php
$pairs = [
    ['9223372036854775807', '9223372036854775808'],
    ['-9223372036854775808', '-9223372036854775809'],
    ['999223372036854775807', '999223372036854775808'],
    ['+000999223372036854775807', '999223372036854775807'],
    ['-000000000000000000000', '+0'],
];
foreach ($pairs as [$left, $right]) {
    echo (int) ($left == $right), '/', $left <=> $right, "\n";
}
var_dump(PHP_INT_MAX == '9223372036854775808');
var_dump(PHP_INT_MIN == '-9223372036854775809');
var_dump('1e20' == '100000000000000000000');
"#,
        ),
        concat!(
            "0/-1\n",
            "0/1\n",
            "0/-1\n",
            "0/-1\n",
            "1/0\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
        ),
    );
}

#[test]
fn malformed_ini_fallback_reports_the_php_parser_expectation() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo $message, "\n";
    return true;
});
var_dump(parse_ini_string('[${ '));
"#,
        ),
        concat!(
            "Warning: syntax error, unexpected end of file, expecting TC_FALLBACK or '}' in Unknown on line 1\n",
            " in  on line 0\n",
            "bool(false)\n",
        ),
    );
}

#[test]
fn never_arrow_functions_defer_the_return_error_until_invocation() {
    assert_eq!(
        run_php(
            r#"<?php
$throws = fn(): never => throw new Exception('expected');
try { $throws(); } catch (Exception $error) { echo $error->getMessage(), "\n"; }
try { assert((fn(): never => 42) && false); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "expected\nassert((fn(): never => 42) && false)\n",
    );
}

#[test]
fn dynamic_property_assignment_reports_rhs_before_an_undefined_simple_name() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo $message, "\n"; return true; });
#[AllowDynamicProperties]
class Target {
    public function __set($name, $value) { $this->$property = $rhs; }
}
$target = new Target;
$name = '';
$target->$name = 1;
"#,
        ),
        "Undefined variable $rhs\nUndefined variable $property\n",
    );
}

#[test]
fn error_handler_scope_introspection_observes_argument_count_first() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler('func_get_args');
try { echo $undefined; } catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "func_get_args() expects exactly 0 arguments, 4 given\n",
    );
}

#[test]
fn output_buffer_chunk_handlers_observe_each_unhandled_diagnostic() {
    assert_eq!(
        run_php(
            r#"<?php
$calls = 0;
$value = null;
ob_start(function ($buffer) use (&$value, &$calls) { ++$calls; return ''; }, 1);
$value .= [];
$value .= [];
ob_end_clean();
echo $calls, "\n";
"#,
        ),
        "3\n",
    );
}

#[test]
fn magic_static_calls_canonicalize_method_names_at_the_first_nul() {
    assert_eq!(
        run_php(
            r#"<?php
class MagicTarget {
    public static function __callStatic($name, $arguments) {
        echo strlen($name), ':', bin2hex($name), '/', count($arguments), "\n";
    }
}
foreach (["", "\0", "\0tail", "head\0tail"] as $name) MagicTarget::$name(1);
"#,
        ),
        "0:/1\n0:/1\n0:/1\n4:68656164/1\n",
    );
}

#[test]
fn repeated_sorts_preserve_recursive_array_reference_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$token = [];
$conditions = [];
for ($i = 0; $i <= 2; $i++) {
    $tokens = $conditions;
    $a[0] =& $a;
    $a = unserialize(serialize($GLOBALS));
    $a[0] =& $a;
    $a = unserialize(serialize($GLOBALS));
    $a[0] =& $a;
    foreach ($a as $value) {
        if ($value == 1) arsort($a);
    }
}
echo "DONE\n";
"#,
        ),
        "DONE\n",
    );
}

#[test]
fn sorting_distinct_recursive_values_still_reports_the_comparison_error() {
    assert_eq!(
        run_php(
            r#"<?php
$left = [];
$left[0] =& $left;
$right = [];
$right[0] =& $right;
$values = [$left, $right];
try { arsort($values); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "Nesting level too deep - recursive dependency?\n",
    );
}

#[test]
fn anonymous_self_property_errors_preserve_the_declared_type_spelling() {
    assert_eq!(
        run_php(
            r#"<?php
$value = new class { public self $property; };
try { $value->property = 0; }
catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "Cannot assign int to property class@anonymous::$property of type self\n",
    );
}

#[test]
fn eval_line_comments_end_at_a_php_closing_tag() {
    assert_eq!(
        run_php(
            r#"<?php
eval('echo "1";//2');
eval('echo 3; //{ 4?>5');
echo "\n";
"#,
        ),
        "135\n",
    );
}
