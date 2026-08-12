/// E2E tests: for, do...while, break, continue, inc/dec, nested loops.
mod common;
use common::{run_php, run_php_expect_error};

// === for loop ===

#[test]
fn test_e2e_for_basic() {
    assert_eq!(
        run_php("<?php for ($i = 0; $i < 5; $i++) { echo $i; }"),
        "01234"
    );
}

#[test]
fn test_e2e_for_sum() {
    assert_eq!(
        run_php("<?php $sum = 0; for ($i = 1; $i <= 10; $i++) { $sum = $sum + $i; } echo $sum;"),
        "55"
    );
}

#[test]
fn test_e2e_for_decrement() {
    assert_eq!(
        run_php("<?php for ($i = 5; $i > 0; $i--) { echo $i; }"),
        "54321"
    );
}

#[test]
fn test_e2e_for_no_body_iterations() {
    assert_eq!(
        run_php("<?php for ($i = 10; $i < 5; $i++) { echo $i; } echo 'done';"),
        "done"
    );
}

#[test]
fn test_e2e_for_with_function() {
    assert_eq!(
        run_php(
            "<?php function double($x) { return $x * 2; } for ($i = 1; $i <= 3; $i++) { echo double($i); }"
        ),
        "246"
    );
}

#[test]
fn test_e2e_typed_double_call_accumulation() {
    assert_eq!(
        run_php(
            "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } $limit = 100; $scale = 2.0; $total = 0.0; for ($i = 0; $i < $limit; ++$i) { $total += calculateFloat(1.5, 2.5, $scale); } echo $i . ':' . $total;"
        ),
        "100:600"
    );
}

#[test]
fn test_e2e_conditional_typed_double_call_accumulation() {
    assert_eq!(
        run_php(
            "<?php function conditionalFloat(float $value, float $pivot): float { if ($value < $pivot) { return ($value * 1.5) + 2.0; } return ($value * 0.5) - 1.0; } $total = 0.0; for ($i = 0; $i < 100; $i++) { $total += conditionalFloat($i * 0.5, 25.0); } echo $i . ':' . $total;"
        ),
        "100:1900"
    );
}

#[test]
fn test_e2e_composed_conditional_typed_double_call_accumulation() {
    assert_eq!(
        run_php(
            "<?php function conditionalFloat(float $value, float $pivot): float { if ($value < $pivot) { return ($value * 1.5) + 2.0; } return ($value * 0.5) - 1.0; } function composedFloat(float $value, float $pivot): float { return (conditionalFloat($value, $pivot) * 1.25) + 3.0; } $total = 0.0; for ($i = 0; $i < 100; $i++) { $total += composedFloat($i * 0.5, 25.0); } echo $i . ':' . $total;"
        ),
        "100:2675"
    );
}

#[test]
fn test_e2e_composed_conditional_typed_double_method_accumulation() {
    assert_eq!(
        run_php(
            "<?php class FloatPipeline { public function conditional(float $value, float $pivot): float { if ($value < $pivot) { return ($value * 1.5) + 2.0; } return ($value * 0.5) - 1.0; } public function composed(float $value, float $pivot): float { return ($this->conditional($value, $pivot) * 1.25) + 3.0; } } $pipeline = new FloatPipeline(); $total = 0.0; for ($i = 0; $i < 100; $i++) { $total += $pipeline->composed($i * 0.5, 25.0); } echo $i . ':' . $total;"
        ),
        "100:2675"
    );
}

#[test]
fn test_e2e_conditional_typed_double_method_accumulation() {
    assert_eq!(
        run_php(
            "<?php class ConditionalFloat { public function apply(float $value, float $pivot): float { $scaled = $value * 1.0; if ($scaled < $pivot) { $result = ($scaled * 1.5) + 2.0; return $result; } $result = ($scaled * 0.5) - 1.0; return $result; } } $calculator = new ConditionalFloat(); $total = 0.0; for ($i = 0; $i < 100; $i++) { $total += $calculator->apply($i * 0.5, 25.0); } echo $i . ':' . $total;"
        ),
        "100:1900"
    );
}

#[test]
fn test_e2e_conditional_typed_double_relations_and_truthiness() {
    assert_eq!(
        run_php(
            "<?php function eqFloat(float $a, float $b): float { if ($a == $b) { return 1.0; } return -1.0; } function neFloat(float $a, float $b): float { if ($a != $b) { return 2.0; } return -2.0; } function leFloat(float $a, float $b): float { if ($a <= $b) { return 3.0; } return -3.0; } function truthyFloat(float $value): float { if ($value) { return 4.0; } return -4.0; } echo eqFloat(2.0, 2.0) . ':' . eqFloat(2.0, 3.0) . '|' . neFloat(2.0, 3.0) . ':' . neFloat(2.0, 2.0) . '|' . leFloat(2.0, 2.0) . ':' . leFloat(3.0, 2.0) . '|' . truthyFloat(1.0) . ':' . truthyFloat(-0.0);"
        ),
        "1:-1|2:-2|3:-3|4:-4"
    );
}

#[test]
fn test_e2e_conditional_typed_double_side_exit_replays_only_selected_edge() {
    assert_eq!(
        run_php(
            "<?php function selectiveDivide(float $value, float $divisor): float { if ($value < 0.0) { return 8.0 / $divisor; } return 3.0; } echo selectiveDivide(1.0, 0.0);"
        ),
        "3"
    );
    let error = run_php_expect_error(
        "<?php function selectiveDivide(float $value, float $divisor): float { if ($value < 0.0) { return 8.0 / $divisor; } return 3.0; } $total = 0.0; for ($i = 0; $i < 100; $i++) { $value = $i == 99 ? -1.0 : 1.0; $total += selectiveDivide($value, 0.0); }",
    );
    assert!(format!("{error:?}").contains("Division by zero"));
}

#[test]
fn test_e2e_typed_double_method_accumulation() {
    assert_eq!(
        run_php(
            "<?php class FloatCalculator { public function calculate(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } } $calculator = new FloatCalculator(); $total = 0.0; for ($i = 0; $i < 100; ++$i) { $total += $calculator->calculate(1.5, 2.5, 2.0); } echo $i . ':' . $total;"
        ),
        "100:600"
    );
}

#[test]
fn test_e2e_typed_double_method_rejects_changed_receiver_class() {
    assert_eq!(
        run_php(
            "<?php class FloatA { public function calculate(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } } class FloatB { public function calculate(float $a, float $b, float $c): float { return (($a + $b) * $c) - 1.0; } } function accumulateFloat($calculator): float { $total = 0.0; for ($i = 0; $i < 100; ++$i) { $total += $calculator->calculate(1.5, 2.5, 2.0); } return $total; } echo accumulateFloat(new FloatA()) . ':' . accumulateFloat(new FloatB());"
        ),
        "600:700"
    );
}

#[test]
fn test_e2e_typed_double_method_division_replays_canonical_error() {
    let error = run_php_expect_error(
        "<?php class FloatDivider { public function divide(float $value, float $divisor): float { return ($value + 1.0) / $divisor; } } $divider = new FloatDivider(); $total = 0.0; for ($i = 0; $i < 5; $i++) { $total += $divider->divide(4.0, $i - 2.0); }",
    );
    assert!(format!("{error:?}").contains("Division by zero"));
}

#[test]
fn test_e2e_typed_double_call_argument_expressions() {
    assert_eq!(
        run_php(
            "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } $limit = 100; $scale = 2.0; $total = 0.0; for ($i = 0; $i < $limit; ++$i) { $total += calculateFloat($i * 0.5, $scale + 1.0, 2.0); } echo $i . ':' . $total;"
        ),
        "100:5350"
    );
}

#[test]
fn test_e2e_nested_typed_double_leaf_accumulation() {
    assert_eq!(
        run_php(
            "<?php function scaleAndShift(float $value, float $scale): float { return ($value * $scale) + 1.0; } function calculateNested(float $value, float $scale): float { return (scaleAndShift($value, $scale) * 0.5) + 2.0; } $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100; $i++) { $total += calculateNested($i * 0.5, $scale); } echo $i . ':' . $total;"
        ),
        "100:2725"
    );
}

#[test]
fn test_e2e_recursive_composed_typed_double_accumulation() {
    assert_eq!(
        run_php(
            "<?php function scaleAndShift(float $value, float $scale): float { return ($value * $scale) + 1.0; } function calculateNested(float $value, float $scale): float { return (scaleAndShift($value, $scale) * 0.5) + 2.0; } function calculateOuter(float $value, float $scale): float { return calculateNested($value, $scale) + 3.0; } $scale = 2.0; $total = 0.0; for ($i = 0; $i < 100; $i++) { $total += calculateOuter($i * 0.5, $scale); } echo $i . ':' . $total;"
        ),
        "100:3025"
    );
}

#[test]
fn test_e2e_nested_typed_double_division_replays_canonical_error() {
    let error = run_php_expect_error(
        "<?php function divideNested(float $value, float $divisor): float { return $value / $divisor; } function calculateNested(float $value, float $divisor): float { return divideNested($value, $divisor) + 1.0; } $total = 0.0; for ($i = 0; $i < 5; $i++) { $total += calculateNested(4.0, $i - 2.0); }",
    );
    assert!(format!("{error:?}").contains("Division by zero"));
}

#[test]
fn test_e2e_typed_double_argument_division_replays_canonical_error() {
    let error = run_php_expect_error(
        "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } $total = 0.0; for ($i = 0; $i < 5; $i++) { $total += calculateFloat(1.0 / ($i - 2.0), 2.0, 3.0); }",
    );
    assert!(format!("{error:?}").contains("Division by zero"));
}

#[test]
fn test_e2e_empty_typed_double_loop_skips_argument_evaluation() {
    assert_eq!(
        run_php(
            "<?php function calculateFloat(float $a, float $b, float $c): float { return (($a + $b) * $c) - 2.0; } $total = 0.0; for ($i = 0; $i < 0; $i++) { $total += calculateFloat(1.0 / 0.0, 2.0, 3.0); } echo $total;"
        ),
        "0"
    );
}

#[test]
fn test_e2e_for_nested() {
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 3; $i++) { for ($j = 0; $j < 2; $j++) { echo $i . $j; } }"
        ),
        "000110112021"
    );
}

#[test]
fn test_e2e_for_string_concat() {
    assert_eq!(
        run_php("<?php $s = ''; for ($i = 1; $i <= 5; $i++) { $s = $s . $i; } echo $s;"),
        "12345"
    );
}

// === increment / decrement ===

#[test]
fn test_e2e_post_inc_value() {
    assert_eq!(run_php("<?php $i = 5; echo $i++;"), "5");
}

#[test]
fn test_e2e_post_inc_side_effect() {
    assert_eq!(run_php("<?php $i = 5; $i++; echo $i;"), "6");
}

#[test]
fn test_e2e_pre_inc_value() {
    assert_eq!(run_php("<?php $i = 5; echo ++$i;"), "6");
}

#[test]
fn test_e2e_pre_dec_value() {
    assert_eq!(run_php("<?php $i = 5; echo --$i;"), "4");
}

#[test]
fn test_e2e_post_dec_value() {
    assert_eq!(run_php("<?php $i = 5; echo $i--;"), "5");
}

#[test]
fn test_e2e_post_dec_side_effect() {
    assert_eq!(run_php("<?php $i = 5; $i--; echo $i;"), "4");
}

// === null inc/dec semantics ===

#[test]
fn test_e2e_null_inc() {
    assert_eq!(run_php("<?php $x = null; $x++; echo $x;"), "1");
}

#[test]
fn test_e2e_null_dec_no_effect() {
    assert_eq!(run_php("<?php $x = null; $x--; echo $x;"), "");
}

#[test]
fn test_e2e_pre_inc_null() {
    assert_eq!(run_php("<?php $x = null; echo ++$x;"), "1");
}

#[test]
fn test_e2e_pre_dec_null() {
    assert_eq!(run_php("<?php $x = null; echo --$x;"), "");
}

#[test]
fn test_e2e_bool_inc_dec_have_no_effect() {
    assert_eq!(
        run_php("<?php $a = true; $b = false; $a++; --$a; ++$b; $b--; var_dump($a, $b);"),
        "bool(true)\nbool(false)\n"
    );
}

// === do...while ===

#[test]
fn test_e2e_do_while_basic() {
    assert_eq!(
        run_php("<?php $i = 0; do { echo $i; $i++; } while ($i < 3);"),
        "012"
    );
}

#[test]
fn test_e2e_do_while_runs_once() {
    assert_eq!(
        run_php("<?php $i = 10; do { echo $i; } while ($i < 5);"),
        "10"
    );
}

#[test]
fn test_e2e_do_while_sum() {
    assert_eq!(
        run_php("<?php $sum = 0; $i = 1; do { $sum += $i; $i++; } while ($i <= 5); echo $sum;"),
        "15"
    );
}

// === break ===

#[test]
fn test_e2e_break_in_while() {
    assert_eq!(
        run_php("<?php $i = 0; while ($i < 10) { if ($i == 3) { break; } echo $i; $i++; }"),
        "012"
    );
}

#[test]
fn test_e2e_break_in_for() {
    assert_eq!(
        run_php("<?php for ($i = 0; $i < 10; $i++) { if ($i == 5) { break; } echo $i; }"),
        "01234"
    );
}

#[test]
fn test_e2e_break_in_do_while() {
    assert_eq!(
        run_php("<?php $i = 0; do { if ($i == 2) { break; } echo $i; $i++; } while ($i < 10);"),
        "01"
    );
}

// === continue ===

#[test]
fn test_e2e_continue_in_while() {
    assert_eq!(
        run_php("<?php $i = 0; while ($i < 5) { $i++; if ($i == 3) { continue; } echo $i; }"),
        "1245"
    );
}

#[test]
fn test_e2e_continue_in_for() {
    assert_eq!(
        run_php("<?php for ($i = 0; $i < 5; $i++) { if ($i == 2) { continue; } echo $i; }"),
        "0134"
    );
}

#[test]
fn test_e2e_continue_in_do_while() {
    assert_eq!(
        run_php("<?php $i = 0; do { $i++; if ($i == 3) { continue; } echo $i; } while ($i < 5);"),
        "1245"
    );
}

// === CR7 regression: break/continue inside function defined in loop ===

#[test]
fn test_e2e_break_in_function_inside_loop_error() {
    // Function body gets its own Compiler — loop_stack should NOT leak into it.
    // break inside a function (even if defined inside a loop) must be a compile error.
    use rphp::compiler::compile::Compiler;
    use rphp::lexer::Lexer;
    use rphp::parser::Parser;

    let tokens = Lexer::new("<?php for ($i = 0; $i < 3; $i++) { function bad() { break; } }")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts);
    assert!(
        result.is_err(),
        "break inside function body should be a compile error"
    );
    assert!(result.err().unwrap().contains("break"));
}

// === nested loop break/continue ===

#[test]
fn test_e2e_break_inner_loop() {
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 3; $i++) { for ($j = 0; $j < 3; $j++) { if ($j == 1) { break; } echo $j; } }"
        ),
        "000"
    );
}

#[test]
fn test_e2e_continue_inner_loop() {
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 2; $i++) { for ($j = 0; $j < 3; $j++) { if ($j == 1) { continue; } echo $j; } }"
        ),
        "0202"
    );
}

// === break N / continue N (multi-level) ===

#[test]
fn test_e2e_break_2_exits_outer_loop() {
    // break 2 inside inner loop exits both loops
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 3; $i++) { for ($j = 0; $j < 3; $j++) { if ($j == 1) { break 2; } echo $j; } } echo 'done';"
        ),
        "0done"
    );
}

#[test]
fn test_e2e_break_2_in_while() {
    assert_eq!(
        run_php(
            "<?php $i = 0; while ($i < 5) { $j = 0; while ($j < 5) { if ($i == 1 && $j == 2) { break 2; } $j++; } $i++; } echo $i . $j;"
        ),
        "12"
    );
}

#[test]
fn test_e2e_continue_2_skips_outer_iteration() {
    // continue 2 inside inner loop continues the outer loop
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 3; $i++) { for ($j = 0; $j < 3; $j++) { if ($j == 1) { continue 2; } echo $i . $j; } echo 'X'; }"
        ),
        "001020"
    );
}

#[test]
fn test_e2e_break_2_switch_in_for() {
    // break 2 inside switch inside for loop — exits the for loop
    // Inspired by php-src/tests/lang/021.phpt
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i <= 5; $i++) { switch ($i) { case 3: break 2; default: echo $i; break; } } echo 'end';"
        ),
        "012end"
    );
}

#[test]
fn test_e2e_continue_2_switch_in_for() {
    // continue 2 inside switch inside for loop — continues the for loop
    assert_eq!(
        run_php(
            "<?php $r = ''; for ($i = 0; $i < 5; $i++) { switch ($i) { case 2: continue 2; default: $r .= $i; break; } } echo $r;"
        ),
        "0134"
    );
}

#[test]
fn test_e2e_break_3_triple_nested() {
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 3; $i++) { for ($j = 0; $j < 3; $j++) { for ($k = 0; $k < 3; $k++) { if ($k == 1) { break 3; } echo $k; } } } echo 'end';"
        ),
        "0end"
    );
}

#[test]
fn test_e2e_break_1_same_as_break() {
    // break 1 is equivalent to break
    assert_eq!(
        run_php("<?php for ($i = 0; $i < 5; $i++) { if ($i == 3) { break 1; } echo $i; }"),
        "012"
    );
}

#[test]
fn test_e2e_continue_1_same_as_continue() {
    assert_eq!(
        run_php("<?php for ($i = 0; $i < 5; $i++) { if ($i == 2) { continue 1; } echo $i; }"),
        "0134"
    );
}

#[test]
fn test_e2e_break_too_deep_compile_error() {
    // break 3 in a 2-deep nesting should fail
    use rphp::compiler::compile::Compiler;
    use rphp::lexer::Lexer;
    use rphp::parser::Parser;

    let tokens =
        Lexer::new("<?php for ($i = 0; $i < 3; $i++) { for ($j = 0; $j < 3; $j++) { break 3; } }")
            .tokenize()
            .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts);
    assert!(
        result.is_err(),
        "break 3 in 2-deep nesting should be a compile error"
    );
}

#[test]
fn test_e2e_continue_too_deep_compile_error() {
    use rphp::compiler::compile::Compiler;
    use rphp::lexer::Lexer;
    use rphp::parser::Parser;

    let tokens = Lexer::new("<?php while (1) { continue 2; }")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let result = Compiler::new().compile(&stmts);
    assert!(
        result.is_err(),
        "continue 2 in 1-deep nesting should be a compile error"
    );
}

#[test]
fn test_e2e_do_while_break_2() {
    assert_eq!(
        run_php(
            "<?php $i = 0; while ($i < 3) { $j = 0; do { if ($j == 1) { break 2; } echo $j; $j++; } while ($j < 3); $i++; } echo 'end';"
        ),
        "0end"
    );
}

#[test]
fn test_e2e_continue_2_do_while_in_for() {
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 3; $i++) { $j = 0; do { if ($j == 1) { continue 2; } echo $i . $j; $j++; } while ($j < 3); echo 'X'; }"
        ),
        "001020"
    );
}
