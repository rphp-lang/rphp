/// E2E tests: preg_match and preg_replace — regex functions.

mod common;
use common::run_php;

// === preg_match — basic (no captures) ===

#[test]
fn test_preg_match_basic_match() {
    assert_eq!(run_php("<?php echo preg_match('/hello/', 'hello world');"), "1");
}

#[test]
fn test_preg_match_basic_no_match() {
    assert_eq!(run_php("<?php echo preg_match('/xyz/', 'hello world');"), "0");
}

// === preg_match — with capture groups ===

#[test]
fn test_preg_match_captures_debug() {
    // First check: does preg_match return 1 when captures arg is provided?
    assert_eq!(
        run_php("<?php $r = preg_match('/hello/', 'hello world', $m); echo $r;"),
        "1"
    );
}

#[test]
fn test_preg_match_captures_full_match() {
    assert_eq!(
        run_php("<?php preg_match('/h(e)(l+)o/', 'hello', $m); echo $m[0];"),
        "hello"
    );
}

#[test]
fn test_preg_match_captures_group1() {
    assert_eq!(
        run_php("<?php preg_match('/h(e)(l+)o/', 'hello', $m); echo $m[1];"),
        "e"
    );
}

#[test]
fn test_preg_match_captures_group2() {
    assert_eq!(
        run_php("<?php preg_match('/h(e)(l+)o/', 'hello', $m); echo $m[2];"),
        "ll"
    );
}

#[test]
fn test_preg_match_captures_count() {
    assert_eq!(
        run_php("<?php preg_match('/h(e)(l+)o/', 'hello', $m); echo count($m);"),
        "3"
    );
}

#[test]
fn test_preg_match_no_match_empty_captures() {
    assert_eq!(
        run_php("<?php preg_match('/xyz/', 'hello', $m); echo count($m);"),
        "0"
    );
}

// === preg_replace — basic ===

#[test]
fn test_preg_replace_basic() {
    assert_eq!(
        run_php("<?php echo preg_replace('/world/', 'PHP', 'hello world');"),
        "hello PHP"
    );
}

#[test]
fn test_preg_replace_no_match() {
    assert_eq!(
        run_php("<?php echo preg_replace('/xyz/', 'PHP', 'hello world');"),
        "hello world"
    );
}

#[test]
fn test_preg_replace_all_occurrences() {
    assert_eq!(
        run_php("<?php echo preg_replace('/o/', '0', 'foo boo');"),
        "f00 b00"
    );
}

#[test]
fn test_preg_replace_with_backreference() {
    assert_eq!(
        run_php("<?php echo preg_replace('/(\\w+)@(\\w+)/', '$1 at $2', 'user@host');"),
        "user at host"
    );
}

// === Case-insensitive flag ===

#[test]
fn test_preg_match_case_insensitive() {
    assert_eq!(
        run_php("<?php echo preg_match('/hello/i', 'HELLO WORLD');"),
        "1"
    );
}

#[test]
fn test_preg_replace_case_insensitive() {
    assert_eq!(
        run_php("<?php echo preg_replace('/hello/i', 'hi', 'Hello World');"),
        "hi World"
    );
}

// === Character classes and quantifiers ===

#[test]
fn test_preg_match_digit_class() {
    assert_eq!(
        run_php("<?php echo preg_match('/\\d+/', 'abc123def');"),
        "1"
    );
}

#[test]
fn test_preg_match_digit_captures() {
    assert_eq!(
        run_php("<?php preg_match('/(\\d+)/', 'abc123def', $m); echo $m[1];"),
        "123"
    );
}

#[test]
fn test_preg_replace_digit_class() {
    assert_eq!(
        run_php("<?php echo preg_replace('/\\d+/', '#', 'abc123def456');"),
        "abc#def#"
    );
}

#[test]
fn test_preg_match_word_boundary() {
    assert_eq!(
        run_php("<?php echo preg_match('/\\bworld\\b/', 'hello world');"),
        "1"
    );
}

#[test]
fn test_preg_match_anchors() {
    assert_eq!(
        run_php("<?php echo preg_match('/^hello/', 'hello world');"),
        "1"
    );
}

#[test]
fn test_preg_match_anchors_no_match() {
    assert_eq!(
        run_php("<?php echo preg_match('/^world/', 'hello world');"),
        "0"
    );
}

#[test]
fn test_preg_match_quantifiers() {
    assert_eq!(
        run_php("<?php preg_match('/a{2,4}/', 'baaab', $m); echo $m[0];"),
        "aaa"
    );
}

#[test]
fn test_preg_replace_character_class() {
    assert_eq!(
        run_php("<?php echo preg_replace('/[aeiou]/', '*', 'hello');"),
        "h*ll*"
    );
}

// === Invalid pattern returns false, not fatal ===

#[test]
fn test_preg_match_invalid_pattern_returns_false() {
    // PHP returns false for invalid patterns, not a fatal error
    assert_eq!(
        run_php("<?php var_dump(preg_match('/(/', 'a'));"),
        "bool(false)\n"
    );
}

#[test]
fn test_preg_match_unknown_modifier_returns_false() {
    assert_eq!(
        run_php("<?php var_dump(preg_match('/a/z', 'a'));"),
        "bool(false)\n"
    );
}

#[test]
fn test_preg_match_paired_delimiter_malformed() {
    // {a}b} — 'b' is unknown modifier, returns false like PHP
    assert_eq!(
        run_php(r#"<?php var_dump(preg_match('{a}b}', 'a'));"#),
        "bool(false)\n"
    );
}

// === Named capture groups in $matches ===

#[test]
fn test_preg_match_named_group_in_matches() {
    assert_eq!(
        run_php(r#"<?php preg_match('/(?P<name>abc)/', 'abc', $m); echo $m['name'];"#),
        "abc"
    );
}

// === Named backreferences ===

#[test]
fn test_preg_match_named_backref_k() {
    assert_eq!(
        run_php(r#"<?php echo preg_match('/(?<x>a)\k<x>/', 'aa');"#),
        "1"
    );
}

#[test]
fn test_preg_match_named_backref_p_equals() {
    assert_eq!(
        run_php(r#"<?php echo preg_match('/(?P<x>a)(?P=x)/', 'aa');"#),
        "1"
    );
}
