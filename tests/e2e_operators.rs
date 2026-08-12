/// E2E tests: logical operators, ternary, compound assignment, comments.
mod common;
use common::run_php;

use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;

// === Logical operators ===

#[test]
fn test_e2e_and_both_true() {
    assert_eq!(
        run_php("<?php if (1 && 1) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

#[test]
fn test_e2e_and_left_false() {
    assert_eq!(
        run_php("<?php if (0 && 1) { echo \"yes\"; } else { echo \"no\"; }"),
        "no"
    );
}

#[test]
fn test_e2e_and_right_false() {
    assert_eq!(
        run_php("<?php if (1 && 0) { echo \"yes\"; } else { echo \"no\"; }"),
        "no"
    );
}

#[test]
fn test_e2e_or_both_false() {
    assert_eq!(
        run_php("<?php if (0 || 0) { echo \"yes\"; } else { echo \"no\"; }"),
        "no"
    );
}

#[test]
fn test_e2e_or_left_true() {
    assert_eq!(
        run_php("<?php if (1 || 0) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

#[test]
fn test_e2e_or_right_true() {
    assert_eq!(
        run_php("<?php if (0 || 1) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

#[test]
fn test_e2e_not_true() {
    assert_eq!(
        run_php("<?php if (!0) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

#[test]
fn test_e2e_not_false() {
    assert_eq!(
        run_php("<?php if (!1) { echo \"yes\"; } else { echo \"no\"; }"),
        "no"
    );
}

#[test]
fn test_e2e_and_short_circuit() {
    assert_eq!(
        run_php(
            "<?php\nfunction side() { echo \"SIDE\"; return 1; }\nif (0 && side()) { echo \"yes\"; }"
        ),
        ""
    );
}

#[test]
fn test_e2e_or_short_circuit() {
    assert_eq!(
        run_php(
            "<?php\nfunction side() { echo \"SIDE\"; return 1; }\nif (1 || side()) { echo \"yes\"; }"
        ),
        "yes"
    );
}

#[test]
fn test_e2e_logical_complex() {
    assert_eq!(
        run_php("<?php if ((1 && 0) || (0 || 1)) { echo \"yes\"; } else { echo \"no\"; }"),
        "yes"
    );
}

// === Ternary operator ===

#[test]
fn test_e2e_ternary_true() {
    assert_eq!(run_php("<?php echo 1 ? \"yes\" : \"no\";"), "yes");
}

#[test]
fn test_e2e_ternary_false() {
    assert_eq!(run_php("<?php echo 0 ? \"yes\" : \"no\";"), "no");
}

#[test]
fn test_e2e_ternary_variable() {
    assert_eq!(
        run_php("<?php $x = 5; echo $x > 3 ? \"big\" : \"small\";"),
        "big"
    );
}

#[test]
fn test_e2e_ternary_nested() {
    assert_eq!(
        run_php("<?php $x = 0; $y = 1; echo $x ? \"a\" : ($y ? \"b\" : \"c\");"),
        "b"
    );
}

#[test]
fn test_e2e_ternary_in_assignment() {
    assert_eq!(
        run_php("<?php $x = 10; $y = $x > 5 ? $x * 2 : $x; echo $y;"),
        "20"
    );
}

#[test]
fn test_e2e_ternary_with_function() {
    assert_eq!(
        run_php(
            "<?php\nfunction double($n) { return $n * 2; }\n$x = 3;\necho $x > 2 ? double($x) : $x;"
        ),
        "6"
    );
}

#[test]
fn test_e2e_nested_ternary_error() {
    let tokens = Lexer::new("<?php echo 1 ? 2 : 3 ? 4 : 5;")
        .tokenize()
        .unwrap();
    let result = Parser::new(tokens).parse();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unparenthesized"));
}

#[test]
fn test_e2e_parenthesized_ternary_ok() {
    assert_eq!(run_php("<?php echo (1 ? 2 : 0) ? 4 : 5;"), "4");
}

// === Compound assignment ===

#[test]
fn test_e2e_plus_assign() {
    assert_eq!(run_php("<?php $x = 10; $x += 5; echo $x;"), "15");
}

#[test]
fn test_e2e_minus_assign() {
    assert_eq!(run_php("<?php $x = 10; $x -= 3; echo $x;"), "7");
}

#[test]
fn test_e2e_star_assign() {
    assert_eq!(run_php("<?php $x = 4; $x *= 3; echo $x;"), "12");
}

#[test]
fn test_e2e_slash_assign() {
    assert_eq!(run_php("<?php $x = 12; $x /= 4; echo $x;"), "3");
}

#[test]
fn test_e2e_percent_assign() {
    assert_eq!(run_php("<?php $x = 10; $x %= 3; echo $x;"), "1");
}

#[test]
fn test_e2e_dot_assign() {
    assert_eq!(
        run_php("<?php $x = \"hello\"; $x .= \" world\"; echo $x;"),
        "hello world"
    );
}

#[test]
fn test_e2e_compound_assign_in_loop() {
    assert_eq!(
        run_php("<?php $sum = 0; for ($i = 1; $i <= 5; $i++) { $sum += $i; } echo $sum;"),
        "15"
    );
}

#[test]
fn test_e2e_null_coalescing_assign_is_lazy() {
    assert_eq!(
        run_php(
            "<?php function fallback() { echo 'rhs>'; return 9; } $set = 7; $set ??= fallback(); echo $set, '|'; $missing = null; $missing ??= fallback(); echo $missing;"
        ),
        "7|rhs>9"
    );
}

#[test]
fn test_e2e_null_coalescing_assign_array_dimension() {
    assert_eq!(
        run_php(
            "<?php $listeners = []; $listeners[1] ??= 'first'; $listeners[1] ??= 'second'; echo $listeners[1];"
        ),
        "first"
    );
}

#[test]
fn test_e2e_null_coalescing_assign_properties() {
    assert_eq!(
        run_php(
            "<?php class Box { public $value; public static $shared; } $box = new Box(); $box->value ??= 'object'; $box->value ??= 'changed'; Box::$shared ??= 'static'; Box::$shared ??= 'changed'; echo $box->value, '|', Box::$shared;"
        ),
        "object|static"
    );
}

// === Comments ===

#[test]
fn test_e2e_line_comment_slash() {
    assert_eq!(run_php("<?php // this is a comment\necho 42;"), "42");
}

#[test]
fn test_e2e_line_comment_hash() {
    assert_eq!(run_php("<?php # hash comment\necho 99;"), "99");
}

#[test]
fn test_e2e_block_comment() {
    assert_eq!(run_php("<?php /* block\ncomment */ echo 7;"), "7");
}

#[test]
fn test_e2e_inline_block_comment() {
    assert_eq!(run_php("<?php echo /* between */ 5;"), "5");
}

// === CR regression: break/continue outside loop = compile error ===

#[test]
fn test_e2e_break_outside_loop_error() {
    let tokens = Lexer::new("<?php break;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("break"));
}

#[test]
fn test_e2e_continue_outside_loop_error() {
    let tokens = Lexer::new("<?php continue;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("continue"));
}

// === CR7 regression: nested ternary in then-branch is allowed (PHP 8+) ===

#[test]
fn test_e2e_nested_ternary_in_then_branch_ok() {
    // PHP 8 allows nested ternary in then-branch: 1 ? 2 ? 3 : 4 : 5
    // Parses as: 1 ? (2 ? 3 : 4) : 5 → result is 3
    assert_eq!(run_php("<?php echo 1 ? 2 ? 3 : 4 : 5;"), "3");
}

#[test]
fn test_e2e_nested_ternary_in_then_branch_false() {
    // 0 ? 1 ? 2 : 3 : 4 → else-branch → 4
    assert_eq!(run_php("<?php echo 0 ? 1 ? 2 : 3 : 4;"), "4");
}

// === CR5 regression: unterminated block comment ===

#[test]
fn test_e2e_unterminated_block_comment() {
    let result = Lexer::new("<?php /* unterminated").tokenize();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unterminated comment"));
}

// === Extra userland args frame integrity ===

#[test]
fn test_e2e_too_many_args_frame_integrity() {
    assert_eq!(
        run_php("<?php\nfunction add($a) { $copy = $a; return $copy; }\necho add(1, 2);"),
        "1"
    );
}

// ========== Strict comparison (===, !==) ==========

#[test]
fn test_identical_int_int() {
    assert_eq!(run_php("<?php echo 1 === 1 ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_int_string_false() {
    assert_eq!(run_php("<?php echo 1 === '1' ? 'yes' : 'no';"), "no");
}

#[test]
fn test_identical_string_string() {
    assert_eq!(run_php("<?php echo 'abc' === 'abc' ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_null_null() {
    assert_eq!(run_php("<?php echo null === null ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_null_false() {
    assert_eq!(run_php("<?php echo null === false ? 'yes' : 'no';"), "no");
}

#[test]
fn test_identical_true_true() {
    assert_eq!(run_php("<?php echo true === true ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_false_false() {
    assert_eq!(run_php("<?php echo false === false ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_identical_true_one() {
    assert_eq!(run_php("<?php echo true === 1 ? 'yes' : 'no';"), "no");
}

#[test]
fn test_not_identical_basic() {
    assert_eq!(run_php("<?php echo 1 !== '1' ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_not_identical_same_type() {
    assert_eq!(run_php("<?php echo 1 !== 1 ? 'yes' : 'no';"), "no");
}

#[test]
fn test_identical_in_if() {
    assert_eq!(
        run_php(
            "<?php
$x = 0;
if ($x === 0) {
    echo 'zero';
} else {
    echo 'other';
}
"
        ),
        "zero"
    );
}

#[test]
fn test_identical_strpos_false_check() {
    assert_eq!(
        run_php(
            "<?php
$pos = 0;
if ($pos === false) {
    echo 'not found';
} else {
    echo 'found at ' . $pos;
}
"
        ),
        "found at 0"
    );
}

#[test]
fn test_identical_arrays_equal() {
    assert_eq!(
        run_php("<?php echo [1, 2, 3] === [1, 2, 3] ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_identical_arrays_different_values() {
    assert_eq!(
        run_php("<?php echo [1, 2] === [1, 3] ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_identical_arrays_different_length() {
    assert_eq!(
        run_php("<?php echo [1, 2] === [1, 2, 3] ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_identical_arrays_different_keys() {
    assert_eq!(
        run_php("<?php echo ['a' => 1] === ['b' => 1] ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_identical_arrays_nested() {
    assert_eq!(
        run_php("<?php echo [[1, 2], [3]] === [[1, 2], [3]] ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_not_identical_arrays() {
    assert_eq!(run_php("<?php echo [1] !== [1] ? 'yes' : 'no';"), "no");
}

#[test]
fn test_practical_identical_vs_equal() {
    assert_eq!(
        run_php(
            "<?php
$results = '';
if (0 === false) { $results .= 'A'; }
if (0 !== false) { $results .= 'B'; }
if ('' === false) { $results .= 'C'; }
if ('' !== false) { $results .= 'D'; }
if (null === false) { $results .= 'E'; }
if (null !== false) { $results .= 'F'; }
echo $results;
"
        ),
        "BDF"
    );
}

// ========== Comparison with concat on right side ==========

#[test]
fn test_identical_with_concat_rhs() {
    assert_eq!(
        run_php("<?php echo 'xy' === 'x' . 'y' ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_equal_with_concat_rhs() {
    assert_eq!(
        run_php("<?php echo 'ab' == 'a' . 'b' ? 'yes' : 'no';"),
        "yes"
    );
}

// ========== Elvis operator (?:) ==========

#[test]
fn test_elvis_truthy_string() {
    assert_eq!(
        run_php(
            r#"<?php
$name = "PHP";
echo $name ?: "default";
"#
        ),
        "PHP"
    );
}

#[test]
fn test_elvis_falsy_empty_string() {
    assert_eq!(
        run_php(
            r#"<?php
$name = "";
echo $name ?: "default";
"#
        ),
        "default"
    );
}

#[test]
fn test_elvis_falsy_zero() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 0;
echo $x ?: 42;
"#
        ),
        "42"
    );
}

#[test]
fn test_elvis_falsy_null() {
    assert_eq!(
        run_php(
            r#"<?php
$x = null;
echo $x ?: "fallback";
"#
        ),
        "fallback"
    );
}

#[test]
fn test_elvis_truthy_number() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 5;
echo $x ?: 99;
"#
        ),
        "5"
    );
}

#[test]
fn test_elvis_with_function_call() {
    assert_eq!(
        run_php(
            r#"<?php
function getName() { return ""; }
echo getName() ?: "anonymous";
"#
        ),
        "anonymous"
    );
}

#[test]
fn test_elvis_chained_with_parens() {
    assert_eq!(
        run_php(
            r#"<?php
$a = "";
$b = "";
$c = "found";
echo ($a ?: $b) ?: $c;
"#
        ),
        "found"
    );
}

#[test]
fn test_elvis_truthy_array() {
    assert_eq!(
        run_php(
            r#"<?php
$x = "yes";
$y = $x ?: "no";
echo $y;
"#
        ),
        "yes"
    );
}

#[test]
fn test_elvis_in_assignment() {
    assert_eq!(
        run_php(
            r#"<?php
$config = "";
$value = $config ?: "default_value";
echo $value;
"#
        ),
        "default_value"
    );
}

#[test]
fn test_elvis_side_effect_truthy_evaluates_once() {
    // P1 regression: LHS with side effects must be evaluated exactly once
    assert_eq!(
        run_php(
            r#"<?php
function foo() { echo "x"; return 123; }
echo foo() ?: 0;
"#
        ),
        "x123"
    );
}

#[test]
fn test_elvis_side_effect_falsy_evaluates_once() {
    assert_eq!(
        run_php(
            r#"<?php
function bar() { echo "y"; return 0; }
echo bar() ?: 99;
"#
        ),
        "y99"
    );
}
