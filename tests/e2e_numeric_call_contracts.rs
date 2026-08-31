mod common;

use common::run_php;

#[test]
fn numeric_functions_expose_php_85_signatures_and_defaults() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['intval', 'fmod', 'intdiv', 'log', 'pow', 'round'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, '|', $function->getNumberOfParameters(), '|',
        $function->getNumberOfRequiredParameters(), '|',
        (string) $function->getReturnType(), "\n";
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), '|', (string) $parameter->getType(), '|',
            $parameter->isOptional() ? 1 : 0, '|',
            $parameter->isDefaultValueAvailable()
                ? var_export($parameter->getDefaultValue(), true)
                : '-', '|',
            $parameter->isDefaultValueAvailable()
                ? ($parameter->isDefaultValueConstant()
                    ? $parameter->getDefaultValueConstantName()
                    : '-')
                : '-', "\n";
    }
}
"#,
        ),
        concat!(
            "intval|2|1|int\n",
            "value|mixed|0|-|-\n",
            "base|int|1|10|-\n",
            "fmod|2|2|float\n",
            "num1|float|0|-|-\n",
            "num2|float|0|-|-\n",
            "intdiv|2|2|int\n",
            "num1|int|0|-|-\n",
            "num2|int|0|-|-\n",
            "log|2|1|float\n",
            "num|float|0|-|-\n",
            "base|float|1|2.718281828459045|M_E\n",
            "pow|2|2|object|int|float\n",
            "num|mixed|0|-|-\n",
            "exponent|mixed|0|-|-\n",
            "round|3|1|float\n",
            "num|int|float|0|-|-\n",
            "precision|int|1|0|-\n",
            "mode|RoundingMode|int|1|\\RoundingMode::HalfAwayFromZero|RoundingMode::HalfAwayFromZero\n",
        )
    );
}

#[test]
fn reflection_parameter_classifies_constant_defaults_without_function_special_cases() {
    assert_eq!(
        run_php(
            r#"<?php
const SAMPLE_DEFAULT = 7;
function sample_defaults(
    $constant = SAMPLE_DEFAULT,
    $literal = 1,
    $expression = SAMPLE_DEFAULT | 1,
    $null = null
) {}
class SampleBox {
    const VALUE = 9;
    public function defaults($constant = self::VALUE, $class = self::class) {}
}
foreach ([
    new ReflectionFunction('sample_defaults'),
    new ReflectionMethod('SampleBox', 'defaults'),
] as $function) {
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), '|',
            $parameter->isDefaultValueConstant() ? 1 : 0, '|';
        var_export($parameter->getDefaultValueConstantName());
        echo "\n";
    }
}
try {
    (new ReflectionParameter('strlen', 0))->isDefaultValueConstant();
} catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "constant|1|'SAMPLE_DEFAULT'\n",
            "literal|0|NULL\n",
            "expression|0|NULL\n",
            "null|0|NULL\n",
            "constant|1|'self::VALUE'\n",
            "class|0|NULL\n",
            "ReflectionException|Internal error: Failed to retrieve the default value\n",
        )
    );
}

#[test]
fn reflection_parameter_retains_resolved_constant_names_for_functions_and_closures() {
    assert_eq!(
        run_php(
            r#"<?php
namespace ConstantSource { const IMPORTED_VALUE = 4; }
namespace ConstantConsumer {
use const ConstantSource\IMPORTED_VALUE as IMPORTED;
const LOCAL_VALUE = 5;
enum Choice { case First; }
function defaults(
    $local = LOCAL_VALUE,
    $imported = IMPORTED,
    $case = Choice::First,
    $expression = LOCAL_VALUE | 1
) {}
$closure = function ($local = LOCAL_VALUE) {};
foreach ([
    new \ReflectionFunction(__NAMESPACE__ . '\\defaults'),
    new \ReflectionFunction($closure),
] as $function) {
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), '|',
            $parameter->isDefaultValueConstant() ? 1 : 0, '|';
        var_export($parameter->getDefaultValueConstantName());
        echo "\n";
    }
}
}
"#,
        ),
        concat!(
            "local|1|'ConstantConsumer\\\\LOCAL_VALUE'\n",
            "imported|1|'ConstantSource\\\\IMPORTED_VALUE'\n",
            "case|1|'ConstantConsumer\\\\Choice::First'\n",
            "expression|0|NULL\n",
            "local|1|'ConstantConsumer\\\\LOCAL_VALUE'\n",
        )
    );
}

#[test]
fn intval_parses_explicit_and_autodetected_bases() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    ['101tail', 2], ['077', 0], ['0x1f', 0], ['-0b101', 0],
    ['0Xf', 16], ['0b10', 2], ['z', 36], ['10xyz', 36],
] as [$value, $base]) {
    echo intval($value, $base), '|';
}
"#,
        ),
        "5|63|31|-5|15|2|35|1723643|"
    );
}

#[test]
fn intval_preserves_decimal_exponents_invalid_bases_and_saturation() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    ['1e2', 10], ['1e2', 0], ['1e2', 16],
    ['10', 1], ['10', 37], ['077', PHP_INT_MIN],
    ['9223372036854775808', 10], ['-9223372036854775809', 10],
] as [$value, $base]) {
    var_dump(intval($value, $base));
}
"#,
        ),
        concat!(
            "int(100)\n",
            "int(1)\n",
            "int(482)\n",
            "int(0)\n",
            "int(0)\n",
            "int(63)\n",
            "int(9223372036854775807)\n",
            "int(-9223372036854775808)\n",
        )
    );
}

#[test]
fn intval_validates_and_weakly_coerces_only_the_base_parameter() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo "D|$message\n"; });
var_dump(intval('10', 2.9), intval('10', '16'), intval('10', null));
try { intval('10', []); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "D|Implicit conversion from float 2.9 to int loses precision\n",
            "D|intval(): Passing null to parameter #2 ($base) of type int is deprecated\n",
            "int(2)\n",
            "int(16)\n",
            "int(10)\n",
            "TypeError|intval(): Argument #2 ($base) must be of type int, array given\n",
        )
    );
}

#[test]
fn intval_strict_call_keeps_value_mixed_but_requires_an_integer_base() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
var_dump(intval('10', 2), intval(12.9, 10));
try { intval('10', '2'); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "int(2)\n",
            "int(12)\n",
            "TypeError|intval(): Argument #2 ($base) must be of type int, string given\n",
        )
    );
}

#[test]
fn intdiv_truncates_towards_zero_and_throws_for_undefined_results() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([[7, 3], [-7, 3], [7, -3], [-7, -3]] as [$left, $right]) {
    var_dump(intdiv($left, $right));
}
foreach ([[1, 0], [PHP_INT_MIN, -1]] as [$left, $right]) {
    try { intdiv($left, $right); } catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "int(2)\n",
            "int(-2)\n",
            "int(-2)\n",
            "int(2)\n",
            "DivisionByZeroError|Division by zero\n",
            "ArithmeticError|Division of PHP_INT_MIN by -1 is not an integer\n",
        )
    );
}

#[test]
fn intdiv_direct_failures_retain_the_internal_trace_frame() {
    assert_eq!(
        run_php(
            r#"<?php
function divide_by_zero() { intdiv(1, 0); }
function overflow_division() { intdiv(PHP_INT_MIN, -1); }
foreach (['divide_by_zero', 'overflow_division'] as $call) {
    try { $call(); } catch (Throwable $error) {
        $trace = $error->getTrace();
        echo $trace[0]['function'], '|', $trace[1]['function'], "\n";
    }
}
"#,
        ),
        concat!("intdiv|divide_by_zero\n", "intdiv|overflow_division\n",)
    );
}

#[test]
fn intdiv_direct_and_dynamic_calls_share_weak_coercion_and_errors() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo "D|$message\n"; });
var_dump(intdiv('9', 2.9));
$callback = 'intdiv';
var_dump($callback(true, '2'));
try { intdiv([], 2); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
try { intdiv(1, []); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "D|Implicit conversion from float 2.9 to int loses precision\n",
            "int(4)\n",
            "int(0)\n",
            "TypeError|intdiv(): Argument #1 ($num1) must be of type int, array given\n",
            "TypeError|intdiv(): Argument #2 ($num2) must be of type int, array given\n",
        )
    );
}

#[test]
fn intdiv_strict_direct_call_rejects_float_and_string_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
try { intdiv(9.0, 2); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
try { intdiv('9', 2); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "TypeError|intdiv(): Argument #1 ($num1) must be of type int, float given\n",
            "TypeError|intdiv(): Argument #1 ($num1) must be of type int, string given\n",
        )
    );
}

#[test]
fn fmod_preserves_sign_and_ieee_special_values() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(fmod(7.5, 2), fmod(-7.5, 2), fmod(7.5, -2));
foreach ([[1.0, 0.0], [INF, 2.0], [NAN, 2.0]] as [$left, $right]) {
    echo is_nan(fmod($left, $right)) ? 'nan' : 'other', '|';
}
var_dump(fmod(2.0, INF));
"#,
        ),
        concat!(
            "float(1.5)\n",
            "float(-1.5)\n",
            "float(1.5)\n",
            "nan|nan|nan|float(2)\n",
        )
    );
}

#[test]
fn fmod_weak_and_strict_float_boundaries_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo "D|$message\n"; });
var_dump(fmod('7.5', true), fmod(null, 2));
try { fmod([], 2); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "D|fmod(): Passing null to parameter #1 ($num1) of type float is deprecated\n",
            "float(0.5)\n",
            "float(0)\n",
            "TypeError|fmod(): Argument #1 ($num1) must be of type float, array given\n",
        )
    );
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
var_dump(fmod(7, 2));
try { fmod('7', 2); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "float(1)\n",
            "TypeError|fmod(): Argument #1 ($num1) must be of type float, string given\n",
        )
    );
}

#[test]
fn log_supports_natural_and_explicit_bases() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(log(M_E), log(100, 10), log(8, 2));
var_dump(log(0), log(-1), log(8, 1), log(2, INF), log(2, NAN));
"#,
        ),
        concat!(
            "float(1)\n",
            "float(2)\n",
            "float(3)\n",
            "float(-INF)\n",
            "float(NAN)\n",
            "float(NAN)\n",
            "float(0)\n",
            "float(NAN)\n",
        )
    );
}

#[test]
fn log_validates_base_domain_after_float_parameter_conversion() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([0, -0.0, -2] as $base) {
    try { log(8, $base); } catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}
try { log([], new stdClass); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "ValueError|log(): Argument #2 ($base) must be greater than 0\n",
            "ValueError|log(): Argument #2 ($base) must be greater than 0\n",
            "ValueError|log(): Argument #2 ($base) must be greater than 0\n",
            "TypeError|log(): Argument #1 ($num) must be of type float, array given\n",
        )
    );
}

#[test]
fn log_weak_and_strict_calls_apply_the_float_contract() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo "D|$message\n"; });
var_dump(log('8', '2'));
try { log(true, null); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "float(3)\n",
            "D|log(): Passing null to parameter #2 ($base) of type float is deprecated\n",
            "ValueError|log(): Argument #2 ($base) must be greater than 0\n",
        )
    );
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
var_dump(log(8, 2));
try { log('8', 2); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "float(3)\n",
            "TypeError|log(): Argument #1 ($num) must be of type float, string given\n",
        )
    );
}

#[test]
fn pow_keeps_exact_integer_results_and_promotes_overflow() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(pow(2, 10), pow(-2, 3), pow(2, -3));
var_dump(pow(2, 62), pow(2, 63), pow(PHP_INT_MAX, 2));
var_dump(pow(1, PHP_INT_MAX), pow(-1, PHP_INT_MAX), pow(0, PHP_INT_MAX));
var_dump(pow(-2, PHP_INT_MAX), pow(-2, PHP_INT_MAX - 1));
var_dump(pow('2', '3'), pow('2.5', '2'));
"#,
        ),
        concat!(
            "int(1024)\n",
            "int(-8)\n",
            "float(0.125)\n",
            "int(4611686018427387904)\n",
            "float(9.223372036854776E+18)\n",
            "float(8.507059173023462E+37)\n",
            "int(1)\n",
            "int(-1)\n",
            "int(0)\n",
            "float(-INF)\n",
            "float(INF)\n",
            "int(8)\n",
            "float(6.25)\n",
        )
    );
}

#[test]
fn pow_reports_numeric_prefixes_deprecations_and_operand_types() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo "D|$message\n"; });
var_dump(pow('2x', 3), pow(0, -1));
foreach ([['wat', 2], [[], 2], [2, new stdClass]] as [$base, $exponent]) {
    try { pow($base, $exponent); } catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "D|A non-numeric value encountered\n",
            "D|Power of base 0 and negative exponent is deprecated\n",
            "int(8)\n",
            "float(INF)\n",
            "TypeError|Unsupported operand types: string ** int\n",
            "TypeError|Unsupported operand types: array ** int\n",
            "TypeError|Unsupported operand types: int ** stdClass\n",
        )
    );
}

#[test]
fn round_supports_all_legacy_integer_modes() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (range(1, 8) as $mode) {
    echo $mode, '|', round(2.1, 0, $mode), '|', round(-2.1, 0, $mode), '|',
        round(2.5, 0, $mode), '|', round(-2.5, 0, $mode), "\n";
}
"#,
        ),
        concat!(
            "1|2|-2|3|-3\n",
            "2|2|-2|2|-2\n",
            "3|2|-2|2|-2\n",
            "4|2|-2|3|-3\n",
            "5|3|-2|3|-2\n",
            "6|2|-3|2|-3\n",
            "7|2|-2|2|-2\n",
            "8|3|-3|3|-3\n",
        )
    );
}

#[test]
fn round_accepts_every_rounding_mode_enum_case() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (RoundingMode::cases() as $mode) {
    echo $mode->name, '|', round(2.1, 0, $mode), '|', round(-2.1, 0, $mode), "\n";
}
"#,
        ),
        concat!(
            "HalfAwayFromZero|2|-2\n",
            "HalfTowardsZero|2|-2\n",
            "HalfEven|2|-2\n",
            "HalfOdd|2|-2\n",
            "TowardsZero|2|-2\n",
            "AwayFromZero|3|-3\n",
            "NegativeInfinity|2|-3\n",
            "PositiveInfinity|3|-2\n",
        )
    );
}

#[test]
fn round_handles_decimal_ties_negative_precision_and_extremes() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(round(1.005, 2), round(-1.005, 2), round(2.675, 2));
var_dump(round(149, -2), round(150, -2), round(-150, -2));
var_dump(round(1.2345, 1000), round(1.2345, -1000), round(-1.2345, -1000));
var_dump(
    round(1.2345, -23, RoundingMode::PositiveInfinity),
    round(-1.2345, -23, RoundingMode::NegativeInfinity),
    round(1e-24, 24, PHP_ROUND_HALF_UP),
    round(1.005, 14, RoundingMode::NegativeInfinity),
    round(-1.005, 14, RoundingMode::PositiveInfinity),
);
foreach ([PHP_ROUND_HALF_UP, PHP_ROUND_HALF_DOWN, PHP_ROUND_HALF_EVEN, PHP_ROUND_HALF_ODD] as $mode) {
    var_dump(round(0.49999999999999994, 0, $mode));
}
"#,
        ),
        concat!(
            "float(1.01)\n",
            "float(-1.01)\n",
            "float(2.68)\n",
            "float(100)\n",
            "float(200)\n",
            "float(-200)\n",
            "float(1.2345)\n",
            "float(0)\n",
            "float(0)\n",
            "float(1.0E+23)\n",
            "float(-1.0E+23)\n",
            "float(1.0E-24)\n",
            "float(1.005)\n",
            "float(-1.005)\n",
            "float(0)\n",
            "float(0)\n",
            "float(0)\n",
            "float(0)\n",
        )
    );
}

#[test]
fn round_validates_mode_and_weakly_coerces_scalar_parameters() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo "D|$message\n"; });
var_dump(round('2.5', '0', '1'), round(true, null, 1.9));
foreach ([0, 9, [], new stdClass] as $mode) {
    try { round(1.5, 0, $mode); } catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "D|round(): Passing null to parameter #2 ($precision) of type int is deprecated\n",
            "D|Implicit conversion from float 1.9 to int loses precision\n",
            "float(3)\n",
            "float(1)\n",
            "ValueError|round(): Argument #3 ($mode) must be a valid rounding mode (RoundingMode::*)\n",
            "ValueError|round(): Argument #3 ($mode) must be a valid rounding mode (RoundingMode::*)\n",
            "TypeError|round(): Argument #3 ($mode) must be of type RoundingMode|int, array given\n",
            "TypeError|round(): Argument #3 ($mode) must be of type RoundingMode|int, stdClass given\n",
        )
    );
}

#[test]
fn round_strict_calls_widen_ints_but_reject_other_scalar_coercions() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
var_dump(round(25, -1));
foreach ([
    fn() => round('2.5'),
    fn() => round(2.5, '0'),
    fn() => round(2.5, 0, 1.0),
] as $call) {
    try { $call(); } catch (Throwable $error) {
        echo get_class($error), '|', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "float(30)\n",
            "TypeError|round(): Argument #1 ($num) must be of type int|float, string given\n",
            "TypeError|round(): Argument #2 ($precision) must be of type int, string given\n",
            "TypeError|round(): Argument #3 ($mode) must be of type RoundingMode|int, float given\n",
        )
    );
}

#[test]
fn numeric_calls_snapshot_by_value_arguments_before_coercion_callbacks() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 'ff'; $value_ref =& $value;
set_error_handler(function () use (&$value) { $value = '10'; return true; });
var_dump(intval($value_ref, 16.9), $value); restore_error_handler();

$left = 9.9; $right = 2; $left_ref =& $left; $right_ref =& $right;
set_error_handler(function () use (&$right) { $right = 0; return true; });
var_dump(intdiv($left_ref, $right_ref), $right); restore_error_handler();

$left = null; $right = 2; $left_ref =& $left; $right_ref =& $right;
set_error_handler(function () use (&$right) { $right = 0; return true; });
var_dump(fmod($left_ref, $right_ref), $right); restore_error_handler();

$number = null; $base = 2; $number_ref =& $number; $base_ref =& $base;
set_error_handler(function () use (&$base) { $base = 0; return true; });
var_dump(log($number_ref, $base_ref), $base); restore_error_handler();

$number = 2.5; $precision = null; $mode = 1;
$number_ref =& $number; $precision_ref =& $precision; $mode_ref =& $mode;
set_error_handler(function () use (&$mode) { $mode = 2; return true; });
var_dump(round($number_ref, $precision_ref, $mode_ref), $mode); restore_error_handler();

$base = '2x'; $exponent = 3; $base_ref =& $base; $exponent_ref =& $exponent;
set_error_handler(function () use (&$exponent) { $exponent = 2; return true; });
var_dump(pow($base_ref, $exponent_ref), $exponent); restore_error_handler();
"#,
        ),
        concat!(
            "int(255)\n",
            "string(2) \"10\"\n",
            "int(4)\n",
            "int(0)\n",
            "float(0)\n",
            "int(0)\n",
            "float(-INF)\n",
            "int(0)\n",
            "float(3)\n",
            "int(2)\n",
            "int(8)\n",
            "int(2)\n",
        )
    );
}

#[test]
fn numeric_functions_support_named_unpack_and_first_class_calls() {
    assert_eq!(
        run_php(
            r#"<?php
$divide = intdiv(...);
$power = pow(...);
var_dump(
    intval(base: 16, value: 'ff'),
    fmod(num2: 2, num1: 7.5),
    $divide(num2: 3, num1: 8),
    log(base: 2, num: 8),
    $power(exponent: 3, num: 2),
    round(mode: RoundingMode::HalfEven, num: 2.5, precision: 0),
    intval(...['value' => '11', 'base' => 2]),
    round(...['num' => 2.5, 'mode' => 2]),
);
"#,
        ),
        concat!(
            "int(255)\n",
            "float(1.5)\n",
            "int(2)\n",
            "float(3)\n",
            "int(8)\n",
            "float(2)\n",
            "int(3)\n",
            "float(2)\n",
        )
    );
}
