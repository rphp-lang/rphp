/// Tests for null coalescing operator (??)
mod common;
use common::run_php;

#[test]
fn test_null_coalescing_null_var() {
    assert_eq!(run_php("<?php $a = null; echo $a ?? 'default';"), "default");
}

#[test]
fn test_null_coalescing_defined_var() {
    assert_eq!(run_php("<?php $a = 42; echo $a ?? 'default';"), "42");
}

#[test]
fn test_null_coalescing_undefined_var() {
    assert_eq!(run_php("<?php echo $x ?? 'fallback';"), "fallback");
}

#[test]
fn test_null_coalescing_zero_is_not_null() {
    assert_eq!(run_php("<?php $a = 0; echo $a ?? 'default';"), "0");
}

#[test]
fn test_null_coalescing_empty_string_is_not_null() {
    assert_eq!(run_php("<?php $a = ''; echo $a ?? 'default';"), "");
}

#[test]
fn test_null_coalescing_false_is_not_null() {
    assert_eq!(run_php("<?php $a = false; echo $a ?? 'default';"), "");
}

#[test]
fn test_null_coalescing_chain() {
    assert_eq!(
        run_php("<?php $a = null; $b = null; echo $a ?? $b ?? 'end';"),
        "end"
    );
}

#[test]
fn test_null_coalescing_chain_first_wins() {
    assert_eq!(
        run_php("<?php $a = 'first'; $b = 'second'; echo $a ?? $b ?? 'end';"),
        "first"
    );
}

#[test]
fn test_null_coalescing_chain_middle_wins() {
    assert_eq!(
        run_php("<?php $a = null; $b = 'middle'; echo $a ?? $b ?? 'end';"),
        "middle"
    );
}

#[test]
fn test_null_coalescing_with_assignment() {
    assert_eq!(run_php("<?php $a = null; $b = $a ?? 10; echo $b;"), "10");
}

#[test]
fn test_null_coalescing_with_expression() {
    assert_eq!(run_php("<?php $a = null; echo ($a ?? 5) + 3;"), "8");
}

#[test]
fn test_null_coalescing_string_value() {
    assert_eq!(
        run_php("<?php $name = null; echo 'Hello ' . ($name ?? 'World');"),
        "Hello World"
    );
}
