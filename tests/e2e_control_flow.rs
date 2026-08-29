/// E2E tests: if, else, elseif, while, nested conditions.
mod common;
use common::run_php;

// === if/else ===

#[test]
fn test_e2e_if_true() {
    assert_eq!(run_php("<?php if (1) echo 42;"), "42");
}

#[test]
fn test_e2e_if_false() {
    assert_eq!(run_php("<?php if (0) echo 42;"), "");
}

#[test]
fn test_e2e_if_variable() {
    assert_eq!(run_php("<?php $x = 1; if ($x) echo 42;"), "42");
}

#[test]
fn test_e2e_if_zero_variable() {
    assert_eq!(run_php("<?php $x = 0; if ($x) echo 42;"), "");
}

#[test]
fn test_e2e_if_else_true() {
    assert_eq!(run_php("<?php if (1) { echo 1; } else { echo 2; }"), "1");
}

#[test]
fn test_e2e_if_else_false() {
    assert_eq!(run_php("<?php if (0) { echo 1; } else { echo 2; }"), "2");
}

#[test]
fn test_e2e_if_comparison() {
    assert_eq!(run_php("<?php $x = 10; if ($x == 10) echo 42;"), "42");
}

#[test]
fn test_e2e_if_comparison_false() {
    assert_eq!(run_php("<?php $x = 10; if ($x == 20) echo 42;"), "");
}

#[test]
fn test_e2e_if_less_than() {
    assert_eq!(run_php("<?php $x = 5; if ($x < 10) echo 42;"), "42");
}

#[test]
fn test_e2e_nested_if() {
    assert_eq!(
        run_php("<?php $x = 5; if ($x > 0) { if ($x < 10) { echo 42; } }"),
        "42"
    );
}

#[test]
fn test_e2e_if_not_equal() {
    assert_eq!(run_php("<?php $x = 5; if ($x != 10) echo 42;"), "42");
}

#[test]
fn alternate_not_equal_matches_standard_not_equal_and_precedence() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 2;
var_dump(1 <> $value, 1 <> "1", 1 + 2 <> 3, (1 <> 2) === (1 != 2));
$assigned = false;
var_dump(false <> $assigned = true, $assigned);
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(false)\n",
            "bool(false)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn test_e2e_if_greater_equal() {
    assert_eq!(run_php("<?php $x = 10; if ($x >= 10) echo 42;"), "42");
}

#[test]
fn test_e2e_if_less_equal() {
    assert_eq!(run_php("<?php $x = 10; if ($x <= 10) echo 42;"), "42");
}

// === elseif ===

#[test]
fn test_e2e_elseif_first() {
    assert_eq!(
        run_php(
            "<?php $x = 1; if ($x == 1) { echo \"one\"; } elseif ($x == 2) { echo \"two\"; } else { echo \"other\"; }"
        ),
        "one"
    );
}

#[test]
fn test_e2e_elseif_second() {
    assert_eq!(
        run_php(
            "<?php $x = 2; if ($x == 1) { echo \"one\"; } elseif ($x == 2) { echo \"two\"; } else { echo \"other\"; }"
        ),
        "two"
    );
}

#[test]
fn test_e2e_elseif_else() {
    assert_eq!(
        run_php(
            "<?php $x = 3; if ($x == 1) { echo \"one\"; } elseif ($x == 2) { echo \"two\"; } else { echo \"other\"; }"
        ),
        "other"
    );
}

#[test]
fn test_e2e_else_if_two_tokens() {
    assert_eq!(
        run_php(
            "<?php $x = 2; if ($x == 1) { echo \"one\"; } else if ($x == 2) { echo \"two\"; } else { echo \"other\"; }"
        ),
        "two"
    );
}

#[test]
fn test_e2e_elseif_chain() {
    assert_eq!(
        run_php(
            "<?php $x = 3; if ($x == 1) { echo \"a\"; } elseif ($x == 2) { echo \"b\"; } elseif ($x == 3) { echo \"c\"; } else { echo \"d\"; }"
        ),
        "c"
    );
}

// === switch ===

#[test]
fn test_e2e_switch_match_first() {
    assert_eq!(
        run_php(
            "<?php $x = 1; switch ($x) { case 1: echo \"one\"; break; case 2: echo \"two\"; break; }"
        ),
        "one"
    );
}

#[test]
fn test_e2e_switch_match_second() {
    assert_eq!(
        run_php(
            "<?php $x = 2; switch ($x) { case 1: echo \"one\"; break; case 2: echo \"two\"; break; }"
        ),
        "two"
    );
}

#[test]
fn test_e2e_switch_no_match() {
    assert_eq!(
        run_php(
            "<?php $x = 99; switch ($x) { case 1: echo \"one\"; break; case 2: echo \"two\"; break; }"
        ),
        ""
    );
}

#[test]
fn test_e2e_switch_default() {
    assert_eq!(
        run_php(
            "<?php $x = 99; switch ($x) { case 1: echo \"one\"; break; default: echo \"other\"; break; }"
        ),
        "other"
    );
}

#[test]
fn test_e2e_switch_fallthrough() {
    // Without break, cases fall through
    assert_eq!(
        run_php(
            "<?php $x = 1; switch ($x) { case 1: echo \"a\"; case 2: echo \"b\"; case 3: echo \"c\"; }"
        ),
        "abc"
    );
}

#[test]
fn test_e2e_switch_fallthrough_partial() {
    // Match case 2, fall through to 3 but break stops
    assert_eq!(
        run_php(
            "<?php $x = 2; switch ($x) { case 1: echo \"a\"; break; case 2: echo \"b\"; case 3: echo \"c\"; break; }"
        ),
        "bc"
    );
}

#[test]
fn test_e2e_switch_string() {
    assert_eq!(
        run_php(
            "<?php $x = \"hello\"; switch ($x) { case \"hi\": echo 1; break; case \"hello\": echo 2; break; default: echo 3; }"
        ),
        "2"
    );
}

#[test]
fn test_e2e_switch_default_middle() {
    // Default in the middle — still works correctly
    assert_eq!(
        run_php(
            "<?php $x = 99; switch ($x) { case 1: echo \"a\"; break; default: echo \"d\"; break; case 2: echo \"b\"; break; }"
        ),
        "d"
    );
}

#[test]
fn test_e2e_switch_in_loop() {
    assert_eq!(
        run_php(
            "<?php for ($i = 0; $i < 3; $i++) { switch ($i) { case 0: echo \"a\"; break; case 1: echo \"b\"; break; default: echo \"c\"; } }"
        ),
        "abc"
    );
}

#[test]
fn test_e2e_switch_nested_in_loop() {
    // Switch inside a for loop — break only exits the switch, not the loop
    assert_eq!(
        run_php(
            "<?php $r = ''; for ($i = 1; $i <= 4; $i++) { switch ($i) { case 1: $r .= 'a'; break; case 3: $r .= 'c'; break; default: $r .= 'x'; break; } } echo $r;"
        ),
        "axcx"
    );
}

#[test]
fn test_e2e_loop_inside_switch() {
    // While loop inside a switch case
    assert_eq!(
        run_php(
            "<?php $x = 1; switch ($x) { case 1: $i = 0; while ($i < 3) { echo $i; $i++; } break; case 2: echo 'no'; break; }"
        ),
        "012"
    );
}

#[test]
fn test_e2e_switch_nested_switch() {
    // Switch inside switch
    assert_eq!(
        run_php(
            "<?php $a = 1; $b = 2; switch ($a) { case 1: switch ($b) { case 1: echo 'aa'; break; case 2: echo 'ab'; break; } break; case 2: echo 'b'; break; }"
        ),
        "ab"
    );
}

// === CR8 regression: switch edge cases ===

#[test]
fn test_e2e_switch_default_middle_match_later_case() {
    // default in the middle — $x=2 should match case 2, NOT default
    assert_eq!(
        run_php(
            "<?php $x = 2; switch ($x) { case 1: echo 'a'; break; default: echo 'd'; break; case 2: echo 'b'; break; }"
        ),
        "b"
    );
}

#[test]
fn test_e2e_switch_default_first_match_later() {
    // default first — $x=2 should match case 2
    assert_eq!(
        run_php(
            "<?php $x = 2; switch ($x) { default: echo 'd'; break; case 1: echo 'a'; break; case 2: echo 'b'; break; }"
        ),
        "b"
    );
}

#[test]
fn test_e2e_switch_continue_acts_as_break() {
    // PHP: continue inside switch acts as break (with a warning)
    assert_eq!(
        run_php(
            "<?php $x = 1; switch ($x) { case 1: echo 'a'; continue; echo 'X'; case 2: echo 'b'; }"
        ),
        "a"
    );
}

#[test]
fn test_e2e_switch_continue_in_loop_context() {
    // continue inside switch inside loop — should continue the loop (acts as break for switch)
    assert_eq!(
        run_php(
            "<?php $r = ''; for ($i = 0; $i < 3; $i++) { switch ($i) { case 0: $r .= 'skip'; continue; default: $r .= $i; } } echo $r;"
        ),
        "skip12"
    );
}

// === while ===

#[test]
fn test_e2e_while_loop() {
    assert_eq!(
        run_php("<?php $x = 0; while ($x < 3) { echo $x; $x = $x + 1; }"),
        "012"
    );
}

#[test]
fn test_e2e_while_no_iter() {
    assert_eq!(run_php("<?php while (0) { echo 1; }"), "");
}

#[test]
fn test_e2e_while_countdown() {
    assert_eq!(
        run_php("<?php $x = 3; while ($x > 0) { echo $x; $x = $x - 1; }"),
        "321"
    );
}

// === CR9 regression: multiple default branches ===

#[test]
fn test_e2e_switch_double_default_is_deferred_compile_error() {
    use rphp::lexer::Lexer;
    use rphp::parser::{Expr, Parser, Stmt};

    let tokens =
        Lexer::new("<?php switch ($x) { default: echo 'a'; break; default: echo 'b'; break; }")
            .tokenize()
            .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line: 1 }))
            if message == "Switch statements may only contain one default clause"
    ));
}

#[test]
fn test_e2e_switch_default_fallthrough_to_case() {
    // default without break falls through to the next case body
    assert_eq!(
        run_php(
            "<?php $x = 99; switch ($x) { case 1: echo 'a'; break; default: echo 'd'; case 2: echo 'b'; break; }"
        ),
        "db"
    );
}
#[test]
fn test_empty_statements_and_loop_body_are_noops() {
    assert_eq!(
        run_php(
            r#"<?php
;
$i = 0;
while ($i++ < 2);
;;
echo $i;
"#,
        ),
        "3"
    );
}

#[test]
fn test_parenthesized_expression_statement_can_invoke_closure() {
    assert_eq!(
        run_php(
            r#"<?php
(function (int $value): void {
    echo $value + 1;
})(41);
"#,
        ),
        "42"
    );
}
#[test]
fn test_for_supports_comma_separated_sections() {
    assert_eq!(
        run_php(
            "<?php $out = ''; for ($i = 0, $j = 3; $ignored = true, $i < 3; ++$i, --$j) { $out .= $i . $j; } echo $out;"
        ),
        "031221"
    );
}

#[test]
fn test_goto_supports_forward_and_backward_labels() {
    assert_eq!(
        run_php(
            "<?php goto start; echo 'skip'; start: $i = 0; again: echo $i; if (++$i < 3) { goto again; }"
        ),
        "012"
    );
}

#[test]
fn goto_can_jump_across_a_lexically_earlier_return() {
    assert_eq!(
        run_php(
            "<?php goto start; done: return; try { start: echo '1'; goto middle; try { middle: echo '2'; goto caught; } catch (Exception $error) { caught: echo '3'; goto outer; } } catch (Exception $error) { outer: echo '4'; goto done; }",
        ),
        "1234"
    );
}

fn compile_error_with_source(source: &str, file: &str) -> String {
    let tokens = rphp::lexer::Lexer::new(source).tokenize().unwrap();
    let statements = rphp::parser::Parser::new(tokens).parse().unwrap();
    rphp::compiler::compile::Compiler::new()
        .with_source_path(file)
        .compile(&statements)
        .err()
        .expect("program should fail during compilation")
        .message
}

#[test]
fn goto_may_leave_a_loop_and_is_case_insensitive() {
    assert_eq!(
        run_php(
            r#"<?php
while (true) {
    echo "a";
    GOTO complete;
}
echo "unreachable";
complete:
echo "b";
"#,
        ),
        "ab"
    );
}

#[test]
fn goto_cannot_enter_any_loop_or_switch_body() {
    let programs = [
        "<?php\ngoto nested;\nwhile (false) { nested: echo 'bad'; }",
        "<?php\ngoto nested;\ndo { nested: echo 'bad'; } while (false);",
        "<?php\ngoto nested;\nfor (; false;) { nested: echo 'bad'; }",
        "<?php\ngoto nested;\nforeach ([] as $value) { nested: echo $value; }",
        "<?php\ngoto nested;\nswitch (0) { case 0: nested: echo 'bad'; }",
    ];

    for source in programs {
        assert_eq!(
            compile_error_with_source(source, "/fixture/goto-regions.php"),
            "'goto' into loop or switch statement is disallowed in /fixture/goto-regions.php on line 2"
        );
    }
}

#[test]
fn goto_cannot_enter_or_leave_finally() {
    let into = compile_error_with_source(
        "<?php\ngoto nested;\ntry { echo 'try'; } finally { nested: echo 'finally'; }",
        "/fixture/goto-finally.php",
    );
    assert_eq!(
        into,
        "jump into a finally block is disallowed in /fixture/goto-finally.php on line 2"
    );

    let out = compile_error_with_source(
        "<?php\ntry { echo 'try'; } finally {\n    goto complete;\n}\ncomplete: echo 'bad';",
        "/fixture/goto-finally.php",
    );
    assert_eq!(
        out,
        "jump out of a finally block is disallowed in /fixture/goto-finally.php on line 3"
    );
}

#[test]
fn goto_within_the_same_finally_block_remains_valid() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "a";
} finally {
    goto marker;
    echo "unreachable";
    marker: echo "b";
}
"#,
        ),
        "ab"
    );
}

#[test]
fn break_and_continue_may_not_escape_a_finally_region() {
    let break_error = compile_error_with_source(
        "<?php\ndo {\n    try {} finally { break; }\n} while (false);",
        "/fixture/finally-loop-transfer.php",
    );
    assert_eq!(
        break_error,
        "jump out of a finally block is disallowed in /fixture/finally-loop-transfer.php on line 3"
    );

    let continue_error = compile_error_with_source(
        "<?php\nfor ($i = 0; $i < 1; ++$i) {\n    try {} finally { continue; }\n}",
        "/fixture/finally-loop-transfer.php",
    );
    assert_eq!(
        continue_error,
        "jump out of a finally block is disallowed in /fixture/finally-loop-transfer.php on line 3"
    );

    assert_eq!(
        run_php("<?php try {} finally { while (true) { echo 'inside'; break; } }"),
        "inside"
    );
}

#[test]
fn goto_leaving_try_runs_intervening_finally_blocks_in_order() {
    assert_eq!(
        run_php(
            r#"<?php
function backward() {
    $round = 0;
again:
    echo "L", $round;
    try {
        echo "T", $round;
        if ($round++ === 0) {
            goto again;
        }
    } finally {
        echo "F", $round;
    }
}
backward();
echo "|";

try {
    echo "A";
    goto forwardDone;
} finally {
    echo "B";
}
echo "bad";
forwardDone:
echo "C|";

try {
    try {
        echo "D";
        goto nestedDone;
    } finally {
        echo "E";
    }
} finally {
    echo "F";
}
nestedDone:
echo "G|";

try {
    try {
        throw new Exception("enter");
    } catch (Exception $error) {
        echo "H";
        goto catchDone;
    } finally {
        echo "I";
    }
    echo "bad";
} catch (Exception $error) {
    echo "bad";
}
catchDone:
echo "J|";

$iteration = 0;
try {
localLabel:
    echo $iteration++;
    if ($iteration < 2) {
        goto localLabel;
    }
} finally {
    echo "K";
}
"#,
        ),
        "L0T0F1L1T1F2|ABC|DEFG|HIJ|01K"
    );
}

#[test]
fn return_or_throw_in_finally_replaces_a_pending_goto() {
    assert_eq!(
        run_php(
            r#"<?php
function returnWins() {
    try {
        goto jumpTarget;
    } finally {
        echo "R";
        return "S";
    }
jumpTarget:
    return "bad";
}
echo returnWins(), "|";

try {
    try {
        goto throwTarget;
    } finally {
        echo "T";
        throw new Exception("U");
    }
throwTarget:
    echo "bad";
} catch (Exception $error) {
    echo $error->getMessage();
}
"#,
        ),
        "RS|TU"
    );
}

#[test]
fn break_and_continue_leaving_try_run_finally_before_loop_transfer() {
    assert_eq!(
        run_php(
            r#"<?php
while (true) {
    try {
        echo "A";
        break;
    } finally {
        echo "B";
    }
}
echo "C|";

for ($index = 0; $index < 2; $index++) {
    try {
        echo $index;
        continue;
    } finally {
        echo "D";
    }
    echo "bad";
}
echo "|";

while (true) {
    try {
        try {
            echo "E";
            break;
        } finally {
            echo "F";
        }
    } finally {
        echo "G";
    }
}
"#,
        ),
        "ABC|0D1D|EFG"
    );
}

#[test]
fn finally_jump_continuation_does_not_leak_into_a_reused_call_frame() {
    assert_eq!(
        run_php(
            r#"<?php
function visit(bool $jump) {
    try {
        echo "A";
        if ($jump) {
            goto finished;
        }
        echo "B";
    } finally {
        echo "C";
    }
finished:
    echo "D";
}
visit(true);
echo "|";
visit(false);
"#,
        ),
        "ACD|ABCD"
    );
}

#[test]
fn finally_jump_continuation_is_not_published_as_a_global() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "A";
    goto finished;
} finally {
    echo "B";
}
finished:
echo isset($GLOBALS["\0finally_jump"]) ? "bad" : "C";
"#,
        ),
        "ABC"
    );
}

#[test]
fn class_keyword_is_ascii_case_insensitive_without_folding_contextual_names() {
    assert_eq!(
        run_php(
            r#"<?php
function choose($MaTcH) {
    return $MaTcH;
}

ClAsS KeywordProbe {
    public $MaTcH = 'property';

    public function RuN() {
        foreach ([1, 2] as $value) {
            if ($value == 1) {
                continue;
            }
            echo $this->MaTcH, ':', $value, '|';
        }
    }
}

$probe = new KeywordProbe();
$probe->RuN();
echo choose(MaTcH: 'named');
"#,
        ),
        "property:2|named"
    );
}
