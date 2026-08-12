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
