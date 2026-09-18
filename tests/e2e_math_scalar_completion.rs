mod common;

use common::run_php;

#[test]
fn scalar_math_uses_php_numeric_argument_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo "D|$message\n"; });
foreach (["23", "-23", "23.45", true, false, null] as $value) {
    var_dump(abs($value), floor($value), ceil($value));
}
try { abs(""); } catch (Throwable $error) {
    echo get_class($error), '|', $error->getMessage(), "\n";
}
var_dump(abs(PHP_INT_MIN));
"#,
        ),
        concat!(
            "int(23)\nfloat(23)\nfloat(23)\n",
            "int(23)\nfloat(-23)\nfloat(-23)\n",
            "float(23.45)\nfloat(23)\nfloat(24)\n",
            "int(1)\nfloat(1)\nfloat(1)\n",
            "int(0)\nfloat(0)\nfloat(0)\n",
            "D|abs(): Passing null to parameter #1 ($num) of type int|float is deprecated\n",
            "D|floor(): Passing null to parameter #1 ($num) of type int|float is deprecated\n",
            "D|ceil(): Passing null to parameter #1 ($num) of type int|float is deprecated\n",
            "int(0)\nfloat(0)\nfloat(0)\n",
            "TypeError|abs(): Argument #1 ($num) must be of type int|float, string given\n",
            "float(9.223372036854776E+18)\n",
        )
    );
}

#[test]
fn number_format_preserves_amd64_integers_and_decimal_policy() {
    assert_eq!(
        run_php(
            r#"<?php
echo number_format(PHP_INT_MAX, 5), "\n";
echo number_format(PHP_INT_MIN, -5), "\n";
echo number_format(1234.5678, 4, '', ''), "\n";
echo number_format(1234.5678, 4, 0, ','), "\n";
echo number_format(-1.15e-15, 2), "\n";
"#,
        ),
        concat!(
            "9,223,372,036,854,775,807.00000\n",
            "-9,223,372,036,854,800,000\n",
            "12345678\n",
            "1,23405678\n",
            "0.00\n",
        )
    );
}

#[test]
fn fpow_power_diagnostics_and_negative_zero_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) { echo "D|$message\n"; });
var_dump(fpow(0, -1), fpow(-2, 2.1));
var_dump(pow(0, -1), 0 ** -1);
printf("%+.17g|%+.17g\n", round(-0.5, 0, PHP_ROUND_HALF_DOWN), round(-0.5, 0, PHP_ROUND_HALF_EVEN));
"#,
        ),
        concat!(
            "float(INF)\nfloat(NAN)\n",
            "D|Power of base 0 and negative exponent is deprecated\n",
            "D|Power of base 0 and negative exponent is deprecated\n",
            "float(INF)\nfloat(INF)\n",
            "-0|-0\n",
        )
    );
}

#[test]
fn math_surface_exposes_constants_angles_and_random_limit() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([M_LOG2E, M_LOG10E, M_LN2, M_SQRTPI, M_EULER, M_SQRT3] as $value) {
    printf("%.16g|", $value);
}
echo "\n";
var_dump(deg2rad(23), rad2deg(23), getrandmax());
"#,
        ),
        concat!(
            "1.442695040888963|0.4342944819032518|0.6931471805599453|",
            "1.772453850905516|0.5772156649015329|1.732050807568877|\n",
            "float(0.40142572795869574)\n",
            "float(1317.8029288008934)\n",
            "int(2147483647)\n",
        )
    );
}
