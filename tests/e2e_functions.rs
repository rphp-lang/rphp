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
        _eg: &ExecutorGlobals,
    ) {
        let arg = unsafe { (*execute_data).cv(0) };
        let val = arg.as_long().unwrap();
        if !return_value.is_null() {
            unsafe { return_value.write(Value::long(val * 2)) };
        }
    }

    let my_double_func = make_internal_function(my_double_handler, 1, 1);

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
        _eg: &ExecutorGlobals,
    ) {
        let arg = unsafe { (*execute_data).cv(0) };
        let val = arg.as_long().unwrap();
        if !return_value.is_null() {
            unsafe { return_value.write(Value::long(val * 2)) };
        }
    }

    let my_double_func = make_internal_function(my_double_handler, 1, 1);

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
