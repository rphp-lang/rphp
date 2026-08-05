/// E2E tests: user-defined functions, internal functions, argument validation.

mod common;
use common::{run_php, run_php_with_functions, run_php_expect_error, make_eg_with_capture};

use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::compiler::compile::Compiler;
use rphp::compiler::{make_user_function, make_internal_function};
use rphp::vm::execute;
use rphp::vm::frame::ExecuteData;
use rphp::vm::function::FunctionCommon;
use rphp::value::Value;
use rphp::runtime::ExecutorGlobals;

// === Internal function calls ===

#[test]
fn test_e2e_function_call() {
    fn my_double_handler(
        execute_data: *mut ExecuteData,
        return_value: *mut Value,
        _eg: &mut ExecutorGlobals,
    ) -> Result<(), rphp::vm::execute::VmError> {
        let arg = unsafe { (*execute_data).cv(0) };
        let val = arg.as_long().unwrap();
        if !return_value.is_null() {
            unsafe { return_value.write(Value::long(val * 2)) };
        }
        Ok(())
    }

    let my_double_func = make_internal_function(my_double_handler, 1, 1, vec!["value".to_string()]);

    let output = run_php_with_functions(
        "<?php echo my_double(21);",
        |eg| {
            eg.register_function(
                "my_double",
                &my_double_func.common as *const FunctionCommon,
            ).unwrap();
        },
    );
    assert_eq!(output, "42");
}

#[test]
fn test_e2e_variable_in_function_call() {
    fn my_double_handler(
        execute_data: *mut ExecuteData,
        return_value: *mut Value,
        _eg: &mut ExecutorGlobals,
    ) -> Result<(), rphp::vm::execute::VmError> {
        let arg = unsafe { (*execute_data).cv(0) };
        let val = arg.as_long().unwrap();
        if !return_value.is_null() {
            unsafe { return_value.write(Value::long(val * 2)) };
        }
        Ok(())
    }

    let my_double_func = make_internal_function(my_double_handler, 1, 1, vec!["value".to_string()]);

    let output = run_php_with_functions(
        "<?php $x = 21; echo my_double($x);",
        |eg| {
            eg.register_function(
                "my_double",
                &my_double_func.common as *const FunctionCommon,
            ).unwrap();
        },
    );
    assert_eq!(output, "42");
}

// === User-defined functions ===

#[test]
fn test_e2e_user_function_basic() {
    assert_eq!(
        run_php("<?php function double($x) { return $x * 2; } echo double(21);"),
        "42"
    );
}

#[test]
fn test_e2e_user_function_two_args() {
    assert_eq!(
        run_php("<?php function add($a, $b) { return $a + $b; } echo add(20, 22);"),
        "42"
    );
}

#[test]
fn test_e2e_user_function_string() {
    assert_eq!(
        run_php("<?php function greet($name) { return \"Hello \" . $name; } echo greet(\"PHP\");"),
        "Hello PHP"
    );
}

#[test]
fn test_e2e_user_function_no_return() {
    assert_eq!(
        run_php("<?php function noop() { $x = 1; } noop(); echo 42;"),
        "42"
    );
}

#[test]
fn test_e2e_user_function_multiple_calls() {
    assert_eq!(
        run_php("<?php function inc($x) { return $x + 1; } echo inc(1); echo inc(2); echo inc(3);"),
        "234"
    );
}

#[test]
fn test_e2e_user_function_call_other() {
    assert_eq!(
        run_php("<?php function double($x) { return $x * 2; } function quad($x) { return double(double($x)); } echo quad(3);"),
        "12"
    );
}

#[test]
fn test_e2e_user_function_with_local_vars() {
    assert_eq!(
        run_php("<?php function calc($a, $b) { $sum = $a + $b; $product = $a * $b; return $sum + $product; } echo calc(3, 4);"),
        "19"
    );
}

#[test]
fn test_e2e_scalar_long_plan_straight_line_expression() {
    assert_eq!(
        run_php("<?php function calc($a, $b) { return ($a + 1) * ($b - 2); } echo calc(4, 10);"),
        "40"
    );
}

#[test]
fn test_e2e_scalar_long_plan_falls_back_for_double() {
    assert_eq!(
        run_php("<?php function calc($a, $b) { return ($a + 1) * $b; } $v = calc(1.5, 2); echo gettype($v) . ':' . $v;"),
        "double:5"
    );
}

#[test]
fn test_e2e_scalar_long_plan_falls_back_on_overflow() {
    assert_eq!(
        run_php("<?php function inc($value) { return $value + 1; } echo gettype(inc(9223372036854775807));"),
        "double"
    );
}

#[test]
fn test_e2e_deferred_scalar_call_captures_arguments_in_source_order() {
    assert_eq!(
        run_php(r#"<?php
function add($a, $b) { return $a + $b; }
function replace(&$value) { $value = 10; return 2; }
$value = 1;
echo add($value, replace($value)) . ':' . $value;
"#),
        "3:10"
    );
}

#[test]
fn test_e2e_deferred_scalar_call_falls_back_for_double_and_overflow() {
    assert_eq!(
        run_php(r#"<?php
function add($a, $b) { return $a + $b; }
function identity($value) { return $value; }
$double = add(identity(1.5), 2);
$overflow = add(identity(9223372036854775807), 1);
echo gettype($double) . ':' . $double . '|' . gettype($overflow);
"#),
        "double:3.5|double"
    );
}

#[test]
fn test_e2e_deferred_scalar_call_is_cleaned_when_argument_throws() {
    assert_eq!(
        run_php(r#"<?php
function add($a, $b) { return $a + $b; }
function fail() { throw new Exception('stop'); }
try {
    echo add(1, fail());
} catch (Exception $error) {
    echo $error->getMessage();
}
echo ':' . add(2, 3);
"#),
        "stop:5"
    );
}

#[test]
fn test_e2e_composed_scalar_call_supports_multiple_nested_levels() {
    assert_eq!(
        run_php(r#"<?php
function add($a, $b) { return $a + $b; }
function mul($a, $b) { return $a * $b; }
$sum = 0;
for ($i = 0; $i < 20; $i++) { $sum += add(1, add(2, mul(3, 4))); }
echo $sum;
"#),
        "300"
    );
}

#[test]
fn test_e2e_composed_scalar_call_falls_back_transactionally_on_nested_overflow() {
    assert_eq!(
        run_php(r#"<?php
function add($a, $b) { return $a + $b; }
function mul($a, $b) { return $a * $b; }
function calculate($input) { return add(1, mul($input, 2)); }
calculate(2);
$value = calculate(9223372036854775807);
echo gettype($value) . ':' . $value;
"#),
        "double:18446744073709552000"
    );
}

#[test]
fn test_e2e_composed_scalar_body_falls_back_for_double_and_overflow() {
    assert_eq!(
        run_php(r#"<?php
function add1($value) { return $value + 1; }
function twice($value) { return $value * 2; }
function combine($a, $b) { return add1($a) + twice($b); }
combine(1, 2);
$double = combine(1.5, 2);
$overflow = combine(9223372036854775807, 1);
echo gettype($double) . ':' . $double . '|' . gettype($overflow);
"#),
        "double:6.5|double"
    );
}

#[test]
fn test_e2e_composed_scalar_body_does_not_speculate_side_effecting_target() {
    assert_eq!(
        run_php(r#"<?php
function touch($value) { echo 'T'; return $value + 1; }
function twice($value) { return $value * 2; }
function combine($a, $b) { return touch($a) + twice($b); }
echo ':' . combine(2, 3);
"#),
        "T:9"
    );
}

#[test]
fn test_e2e_composed_double_body_uses_exact_direct_path_and_weak_fallback() {
    assert_eq!(
        run_php(r#"<?php
function scaleAndShift(float $value, float $scale): float {
    return ($value * $scale) + 1.0;
}
function calculateNested(float $value, float $scale): float {
    return (scaleAndShift($value, $scale) * 0.5) + 2.0;
}
$exact = calculateNested(2.0, 3.0);
$coerced = calculateNested(2, 3);
echo gettype($exact) . ':' . $exact . '|' . gettype($coerced) . ':' . $coerced;
"#),
        "double:5.5|double:5.5"
    );
}

#[test]
fn test_e2e_recursive_composed_double_body_uses_exact_direct_path_and_weak_fallback() {
    assert_eq!(
        run_php(r#"<?php
function scaleAndShift(float $value, float $scale): float {
    return ($value * $scale) + 1.0;
}
function calculateNested(float $value, float $scale): float {
    return (scaleAndShift($value, $scale) * 0.5) + 2.0;
}
function calculateOuter(float $value, float $scale): float {
    return calculateNested($value, $scale) + 3.0;
}
$exact = calculateOuter(2.0, 3.0);
$coerced = calculateOuter(2, 3);
echo gettype($exact) . ':' . $exact . '|' . gettype($coerced) . ':' . $coerced;
"#),
        "double:8.5|double:8.5"
    );
}

#[test]
fn test_e2e_composed_double_body_accepts_one_conditional_leaf() {
    assert_eq!(
        run_php(r#"<?php
function conditionalLeaf(float $value, float $pivot): float {
    if ($value < $pivot) {
        return ($value * 1.5) + 2.0;
    }
    return ($value * 0.5) - 1.0;
}
function conditionalOuter(float $value, float $pivot): float {
    return conditionalLeaf($value, $pivot) + 3.0;
}
echo conditionalOuter(2.0, 3.0) . ':' . conditionalOuter(4.0, 3.0);
"#),
        "8:4"
    );
}

#[test]
fn test_e2e_composed_double_body_safely_falls_back_for_two_conditional_leaves() {
    assert_eq!(
        run_php(
            r#"<?php
function conditionalLeaf(float $value, float $pivot): float {
    if ($value < $pivot) {
        return ($value * 1.5) + 2.0;
    }
    return ($value * 0.5) - 1.0;
}
function conditionalPair(float $a, float $b, float $pivot): float {
    return conditionalLeaf($a, $pivot) + conditionalLeaf($b, $pivot);
}
echo conditionalPair(2.0, 4.0, 3.0);
"#
        ),
        "6"
    );
}

#[test]
fn test_e2e_recursive_composed_double_depth_budget_falls_back() {
    assert_eq!(
        run_php(r#"<?php
function doubleLeaf(float $value): float { return $value * 2.0; }
function doubleLevel1(float $value): float { return doubleLeaf($value) + 1.0; }
function doubleLevel2(float $value): float { return doubleLevel1($value) + 1.0; }
function doubleLevel3(float $value): float { return doubleLevel2($value) + 1.0; }
function doubleLevel4(float $value): float { return doubleLevel3($value) + 1.0; }
function doubleLevel5(float $value): float { return doubleLevel4($value) + 1.0; }
function doubleLevel6(float $value): float { return doubleLevel5($value) + 1.0; }
$result = doubleLevel6(2.0);
echo gettype($result) . ':' . $result;
"#),
        "double:10"
    );
}

#[test]
fn test_e2e_user_function_scope_isolation() {
    assert_eq!(
        run_php("<?php $x = 10; function foo() { $x = 99; return $x; } echo foo(); echo $x;"),
        "9910"
    );
}

// === Argument validation ===

#[test]
fn test_e2e_too_many_args() {
    let err = run_php_expect_error(
        "<?php function add($a, $b) { return $a + $b; } echo add(1, 2, 3);"
    );
    match err {
        execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Too many arguments"), "Expected 'Too many arguments', got: {}", msg);
        }
        _ => panic!("Expected Fatal error, got: {:?}", err),
    }
}

#[test]
fn test_e2e_too_few_args() {
    let err = run_php_expect_error(
        "<?php function add($a, $b) { return $a + $b; } echo add(1);"
    );
    match err {
        execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Too few arguments"), "Expected 'Too few arguments', got: {}", msg);
        }
        _ => panic!("Expected Fatal error, got: {:?}", err),
    }
}

#[test]
fn test_e2e_redeclare_function() {
    let tokens = Lexer::new("<?php function foo() { return 1; } function foo() { return 2; }").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts).unwrap();
    let main_func = make_user_function(result.main);
    let (mut eg, _buf) = make_eg_with_capture();
    let mut err_msg = None;
    for (name, func) in &result.functions {
        if let Err(e) = eg.register_function(name, &func.common as *const FunctionCommon) {
            err_msg = Some(e);
            break;
        }
    }
    assert!(err_msg.is_some(), "Expected redeclare error");
    assert!(err_msg.unwrap().contains("Cannot redeclare"), "Error should mention 'Cannot redeclare'");
    drop(main_func);
}

#[test]
fn test_e2e_too_many_args_no_corruption() {
    let err = run_php_expect_error(
        "<?php function add($a, $b) { return $a + $b; } echo add(1, 2, 3);"
    );
    match err {
        execute::VmError::Fatal(msg) => {
            assert!(msg.contains("Too many arguments"));
        }
        _ => panic!("Expected Fatal error"),
    }
}
