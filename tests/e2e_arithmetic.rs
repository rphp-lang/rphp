/// E2E tests: echo, arithmetic, precedence, basic assignment.
mod common;
use common::run_php;

#[test]
fn test_e2e_echo_42() {
    assert_eq!(run_php("<?php echo 42;"), "42");
}

#[test]
fn test_e2e_echo_negative() {
    assert_eq!(run_php("<?php echo -1;"), "-1");
}

#[test]
fn test_e2e_echo_add() {
    assert_eq!(run_php("<?php echo 20 + 22;"), "42");
}

#[test]
fn test_e2e_echo_sub() {
    assert_eq!(run_php("<?php echo 50 - 8;"), "42");
}

#[test]
fn test_e2e_assign_echo() {
    assert_eq!(run_php("<?php $a = 42; echo $a;"), "42");
}

#[test]
fn test_e2e_assign_add_echo() {
    assert_eq!(run_php("<?php $a = 20; $b = 22; echo $a + $b;"), "42");
}

#[test]
fn test_e2e_multiple_echo() {
    assert_eq!(run_php("<?php echo 1; echo 2; echo 3;"), "123");
}

#[test]
fn test_e2e_comma_separated_echo_preserves_order() {
    assert_eq!(
        run_php(
            "<?php function emit($value) { echo '[' . $value . ']'; return $value; } echo emit('a'), ':', emit('b');",
        ),
        "[a]a:[b]b",
    );
}

#[test]
fn test_e2e_complex_expression() {
    assert_eq!(run_php("<?php echo 10 + 20 + 12;"), "42");
}

#[test]
fn test_e2e_mul() {
    assert_eq!(run_php("<?php echo 6 * 7;"), "42");
}

#[test]
fn test_e2e_div_exact() {
    assert_eq!(run_php("<?php echo 84 / 2;"), "42");
}

#[test]
fn test_e2e_div_float() {
    assert_eq!(run_php("<?php echo 7 / 2;"), "3.5");
}

#[test]
fn test_e2e_mod() {
    assert_eq!(run_php("<?php echo 10 % 3;"), "1");
}

#[test]
fn arithmetic_operator_errors_are_catchable_and_leave_compound_targets_unchanged() {
    assert_eq!(
        run_php(
            r#"<?php
$division = "12";
try {
    $division /= 0;
} catch (DivisionByZeroError $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
var_dump($division);

$modulo = "7";
try {
    $modulo %= "0";
} catch (DivisionByZeroError $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
var_dump($modulo);

foreach ([-1, 64, 65] as $distance) {
    try {
        var_dump(-3 << $distance, -3 >> $distance);
    } catch (ArithmeticError $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        "DivisionByZeroError:Division by zero\nstring(2) \"12\"\nDivisionByZeroError:Modulo by zero\nstring(1) \"7\"\nArithmeticError:Bit shift by negative number\nint(0)\nint(-1)\nint(0)\nint(-1)\n",
    );
}

#[test]
fn test_e2e_precedence_mul_add() {
    assert_eq!(run_php("<?php echo 2 + 3 * 4;"), "14");
}

#[test]
fn test_e2e_precedence_complex() {
    assert_eq!(run_php("<?php echo 10 - 2 * 3 + 4 / 2;"), "6");
}

#[test]
fn test_e2e_paren_expr() {
    assert_eq!(run_php("<?php echo (2 + 3) * 4;"), "20");
}

#[test]
fn test_e2e_nested_paren() {
    assert_eq!(run_php("<?php echo ((1 + 2) * (3 + 4));"), "21");
}

#[test]
fn test_e2e_integer_overflow_error() {
    let result = rphp::lexer::Lexer::new("<?php echo 99999999999999999999;").tokenize();
    assert!(result.is_err());
}

// ========== Float literals ==========

#[test]
fn test_float_literal_echo() {
    assert_eq!(run_php("<?php echo 3.14;"), "3.14");
}

#[test]
fn test_float_literal_assignment() {
    assert_eq!(run_php("<?php $x = 2.5; echo $x;"), "2.5");
}

#[test]
fn test_float_arithmetic() {
    assert_eq!(run_php("<?php echo 1.5 + 2.5;"), "4");
}

#[test]
fn test_float_mul() {
    assert_eq!(run_php("<?php echo 2.5 * 4.0;"), "10");
}

#[test]
fn test_float_int_mixed() {
    assert_eq!(run_php("<?php echo 3 + 0.14;"), "3.14");
}

#[test]
fn test_arithmetic_coerces_bool_null_and_numeric_strings() {
    assert_eq!(
        run_php("<?php echo (5 - false), ':', (true + 2), ':', (null + 3), ':', ('4.5' + 0.5);"),
        "5:3:3:5"
    );
}

#[test]
fn test_float_comparison() {
    assert_eq!(run_php("<?php echo 3.14 > 3 ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_float_in_array() {
    assert_eq!(run_php("<?php $a = [1.5, 2.5]; echo $a[0] + $a[1];"), "4");
}

#[test]
fn test_negative_float_literal() {
    assert_eq!(run_php("<?php echo -3.14;"), "-3.14");
}

#[test]
fn test_float_scientific_notation() {
    assert_eq!(run_php("<?php echo 1.5e2;"), "150");
}

#[test]
fn test_float_whole_number_display() {
    assert_eq!(run_php("<?php echo 3.0;"), "3");
}

// ========== Unary minus ==========

#[test]
fn test_unary_minus_variable() {
    assert_eq!(run_php("<?php $x = 5; echo -$x;"), "-5");
}

#[test]
fn test_unary_minus_expression() {
    assert_eq!(run_php("<?php $x = 3; $y = 2; echo -($x + $y);"), "-5");
}

#[test]
fn test_unary_minus_in_arithmetic() {
    assert_eq!(run_php("<?php $x = 5; echo 10 + -$x;"), "5");
}

#[test]
fn test_unary_minus_function_result() {
    assert_eq!(
        run_php(
            "<?php
function five() { return 5; }
echo -five();
"
        ),
        "-5"
    );
}

#[test]
fn test_unary_minus_float() {
    assert_eq!(run_php("<?php $x = 3.14; echo -$x;"), "-3.14");
}

#[test]
fn test_unary_minus_double_negation() {
    assert_eq!(run_php("<?php $x = 5; echo -(-$x);"), "5");
}

#[test]
fn test_practical_float_average() {
    assert_eq!(
        run_php(
            "<?php
$values = [10, 20, 30];
$sum = 0.0;
foreach ($values as $v) {
    $sum += $v;
}
echo $sum / count($values);
"
        ),
        "20"
    );
}

#[test]
fn test_practical_unary_minus_abs_manual() {
    assert_eq!(
        run_php(
            "<?php
function my_abs($x) {
    if ($x < 0) {
        return -$x;
    }
    return $x;
}
echo my_abs(-5) . ' ' . my_abs(3);
"
        ),
        "5 3"
    );
}
