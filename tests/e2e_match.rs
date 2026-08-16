/// Tests for match expression
mod common;
use common::{run_php, run_php_with_source_context};

#[test]
fn test_match_basic_int() {
    assert_eq!(
        run_php("<?php $x = 2; echo match($x) { 1 => 'one', 2 => 'two', 3 => 'three' };"),
        "two"
    );
}

#[test]
fn test_match_first_arm() {
    assert_eq!(
        run_php("<?php $x = 1; echo match($x) { 1 => 'one', 2 => 'two' };"),
        "one"
    );
}

#[test]
fn test_match_string() {
    assert_eq!(
        run_php("<?php $s = 'b'; echo match($s) { 'a' => 'alpha', 'b' => 'beta' };"),
        "beta"
    );
}

#[test]
fn test_match_with_default() {
    assert_eq!(
        run_php("<?php $x = 99; echo match($x) { 1 => 'one', 2 => 'two', default => 'other' };"),
        "other"
    );
}

#[test]
fn test_match_result_in_variable() {
    let out = run_php("<?php $x = 3; $r = match($x) { 1 => 10, 2 => 20, 3 => 30 }; echo $r;");
    assert_eq!(out, "30");
}

#[test]
fn test_match_strict_comparison() {
    // match uses === (strict), so "1" !== 1
    assert_eq!(
        run_php(
            "<?php $x = '1'; echo match($x) { 1 => 'int', '1' => 'string', default => 'none' };"
        ),
        "string"
    );
}

#[test]
fn test_match_with_expressions() {
    let out = run_php("<?php $x = 2; echo match($x) { 1 => 1+10, 2 => 2+20, 3 => 3+30 };");
    assert_eq!(out, "22");
}

#[test]
fn test_match_null() {
    assert_eq!(
        run_php("<?php $x = null; echo match($x) { null => 'nil', default => 'other' };"),
        "nil"
    );
}

#[test]
fn test_match_bool() {
    assert_eq!(
        run_php("<?php $x = true; echo match($x) { true => 'yes', false => 'no' };"),
        "yes"
    );
}

#[test]
fn test_match_in_concat() {
    let out = run_php("<?php $x = 1; echo 'Result: ' . match($x) { 1 => 'one', default => '?' };");
    assert_eq!(out, "Result: one");
}

#[test]
fn unmatched_values_report_php_diagnostics_and_creation_lines() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nforeach ([true, false, null, 6, 1.0, 1.5, 1.0E+20, -0.0, NAN, 'text', []] as $value) {\n    try { match($value) {}; } catch (UnhandledMatchError $error) { echo $error, \"\\n\"; }\n}",
            "match_error.php",
            ".",
        ),
        "UnhandledMatchError: Unhandled match case true in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case false in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case NULL in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case 6 in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case 1.0 in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case 1.5 in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case 1.0E+20 in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case -0.0 in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case NAN in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case 'text' in match_error.php:3\nStack trace:\n#0 {main}\nUnhandledMatchError: Unhandled match case of type array in match_error.php:3\nStack trace:\n#0 {main}\n"
    );
}
