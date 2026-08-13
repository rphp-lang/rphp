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
fn test_e2e_switch_double_default_parse_error() {
    // PHP rejects switch with multiple default branches
    use rphp::lexer::Lexer;
    use rphp::parser::Parser;

    let tokens =
        Lexer::new("<?php switch ($x) { default: echo 'a'; break; default: echo 'b'; break; }")
            .tokenize()
            .unwrap();
    let result = Parser::new(tokens).parse();
    assert!(
        result.is_err(),
        "multiple default branches should be a parse error"
    );
    assert!(result.err().unwrap().contains("default"));
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

fn compile_error_with_source(source: &str, file: &str) -> String {
    let tokens = rphp::lexer::Lexer::new(source).tokenize().unwrap();
    let statements = rphp::parser::Parser::new(tokens).parse().unwrap();
    rphp::compiler::compile::Compiler::new()
        .with_source_path(file)
        .compile(&statements)
        .err()
        .expect("program should fail during compilation")
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
