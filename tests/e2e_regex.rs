/// E2E tests: preg_match and preg_replace — regex functions.
mod common;
use common::run_php;

#[test]
fn preg_split_supports_delimiter_and_offset_flags() {
    assert_eq!(
        run_php(
            "<?php $parts = preg_split('/(:)/', ':a::', -1, PREG_SPLIT_DELIM_CAPTURE | PREG_SPLIT_NO_EMPTY); echo implode(',', $parts), '|'; foreach (preg_split('/(:)/', 'a:b', -1, PREG_SPLIT_DELIM_CAPTURE | PREG_SPLIT_OFFSET_CAPTURE) as $part) echo $part[0], '@', $part[1], ',';"
        ),
        ":,a,:,:|a@0,:@1,b@2,"
    );
}

#[test]
fn preg_match_supports_define_blocks_and_named_subroutine_calls() {
    assert_eq!(
        run_php(
            r#"<?php $pattern = '/(?(DEFINE)(?<cn>[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*+))(?(DEFINE)(?<fqcn>(?&cn)(?:\\\\(?&cn))*+))^\??(?&fqcn)(?:(?:\|(?&fqcn))*+|(?:&(?&fqcn))*+)$/'; foreach (['?Router', 'App\\Router|Psr\\Log\\LoggerInterface', 'bad-name'] as $type) echo preg_match($pattern, $type);"#
        ),
        "110"
    );
}

// === preg_match — basic (no captures) ===

#[test]
fn test_preg_match_basic_match() {
    assert_eq!(
        run_php("<?php echo preg_match('/hello/', 'hello world');"),
        "1"
    );
}

#[test]
fn test_preg_match_basic_no_match() {
    assert_eq!(
        run_php("<?php echo preg_match('/xyz/', 'hello world');"),
        "0"
    );
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

#[test]
fn test_preg_replace_applies_pattern_and_replacement_arrays_in_order() {
    assert_eq!(
        run_php(
            r#"<?php
$patterns = ['/([A-Z]+)([A-Z][a-z])/', '/([a-z\d])([A-Z])/'];
$replacements = ['\\1_\\2', '\\1_\\2'];
echo strtolower(preg_replace($patterns, $replacements, 'FrameworkBundle')), '|';
$result = preg_replace(['/a/', '/b/', '/c/'], ['A'], ['named' => 'abc', 4 => 'cab']);
echo $result['named'], ':', $result[4];
"#,
        ),
        "framework_bundle|A:A"
    );
}

#[test]
fn preg_match_supports_offset_capture_and_start_offset() {
    let out = run_php(
        r#"<?php
preg_match('/(?<word>[a-z]+)(\d+)?/', '00 alpha 42 beta', $matches, PREG_OFFSET_CAPTURE, 3);
echo $matches[0][0], ':', $matches[0][1], '|';
echo $matches['word'][0], ':', $matches['word'][1], '|';
echo $matches[2][0], ':', $matches[2][1], '|';
preg_match('/beta/', '00 alpha 42 beta', $tail, PREG_OFFSET_CAPTURE, -4);
echo $tail[0][0], ':', $tail[0][1];
"#,
    );
    assert_eq!(out, "alpha:3|alpha:3|:-1|beta:12");
}

#[test]
fn preg_replace_supports_limit_and_total_count() {
    let out = run_php(
        r#"<?php
$count = -1;
echo preg_replace('/a/', 'x', 'aaaa', 2, $count), ':', $count, '|';
$count = -1;
$result = preg_replace(['/a/', '/b/'], ['x', 'y'], ['first' => 'aab', 4 => 'abb'], 1, $count);
echo $result['first'], ':', $result[4], ':', $count;
"#,
    );
    assert_eq!(out, "xxaa:2|xay:xyb:4");
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

#[test]
fn test_preg_match_branch_reset_and_mark_capture() {
    assert_eq!(
        run_php(
            r#"<?php preg_match('{^(?|/a/([a-z]+)(*:28)|/b/([0-9]+)(*:42))$}D', '/b/12', $m); echo $m[1], ':', $m['MARK'];"#
        ),
        "12:42"
    );
}

#[test]
fn test_preg_match_all_publishes_branch_marks() {
    assert_eq!(
        run_php(
            r#"<?php preg_match_all('{(?|/a/([a-z]+)(*:28)|/b/([0-9]+)(*:42))}D', '/a/x /b/12', $m); echo $m[1][0], ':', $m[1][1], ':', $m['MARK'][0], ':', $m['MARK'][1];"#
        ),
        "x:12:28:42"
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

// === Repeated matches and UTF-8 offsets ===

#[test]
fn test_preg_match_all_preserves_utf8_and_named_captures() {
    assert_eq!(
        run_php(
            r#"<?php
            $count = preg_match_all('/(?P<letter>ž|č)(?P<digit>\d)/', '🙂 ž1 x č2', $m);
            echo $count . '|' . implode(',', $m[0]) . '|' . implode(',', $m['letter']) . '|' . implode(',', $m[2]);
            "#,
        ),
        "2|ž1,č2|ž,č|1,2"
    );
}

#[test]
fn test_preg_match_all_set_order_with_named_captures() {
    assert_eq!(
        run_php(
            r#"<?php
            $count = preg_match_all('/(?P<letter>ž|č)(?P<digit>\d)/', '🙂 ž1 x č2', $m, PREG_SET_ORDER);
            echo $count . '|' . $m[0][0] . ':' . $m[0]['letter'] . ':' . $m[0][2]
                . '|' . $m[1][0] . ':' . $m[1]['letter'] . ':' . $m[1][2];
            "#,
        ),
        "2|ž1:ž:1|č2:č:2"
    );
}

#[test]
fn test_preg_match_all_set_order_omits_trailing_unmatched_groups() {
    assert_eq!(
        run_php(
            r#"<?php
            preg_match_all('/(?<separator>,)|[^,]+/', 'a,b', $matches, PREG_SET_ORDER);
            foreach ($matches as $row) {
                echo count($row), ':', isset($row['separator']) ? 'yes' : 'no', ':', $row[0], '|';
            }
            "#,
        ),
        "1:no:a|3:yes:,|1:no:b|"
    );
}

#[test]
fn test_preg_match_all_no_match_keeps_pattern_order_shape() {
    assert_eq!(
        run_php(
            r#"<?php
            $count = preg_match_all('/z/', 'abc', $m);
            echo $count . '|' . count($m) . '|' . count($m[0]);
            "#,
        ),
        "0|1|0"
    );
}

#[test]
fn test_preg_replace_callback_preserves_utf8_named_captures() {
    assert_eq!(
        run_php(
            r#"<?php
            function wrap_utf8_match($matches) {
                return '[' . $matches['letter'] . $matches[2] . ']';
            }
            echo preg_replace_callback('/(?P<letter>ž|č)(\d)/', 'wrap_utf8_match', '🙂 ž1 x č2');
            "#,
        ),
        "🙂 [ž1] x [č2]"
    );
}

#[test]
fn test_preg_replace_callback_advances_zero_width_utf8_matches() {
    assert_eq!(
        run_php(
            r#"<?php
            function insert_marker($matches) {
                return 'X';
            }
            echo preg_replace_callback('/(?=a)/', 'insert_marker', 'aéa');
            "#,
        ),
        "XaéXa"
    );
}

#[test]
fn test_preg_replace_callback_builds_variable_length_output_in_order() {
    assert_eq!(
        run_php(
            r#"<?php
            function expand_ordered_match($matches) {
                return '<' . $matches[0] . $matches[0] . '>';
            }
            $result = preg_replace_callback('/[a-z]+/', 'expand_ordered_match', 'a1bb22c');
            echo $result;
            "#,
        ),
        "<aa>1<bbbb>22<cc>"
    );
}

#[test]
fn test_preg_replace_callback_builds_empty_replacements() {
    assert_eq!(
        run_php(
            r#"<?php
            function delete_match($matches) {
                return '';
            }
            echo preg_replace_callback('/[a-z]+/', 'delete_match', 'a1bb22c');
            "#,
        ),
        "122"
    );
}

#[test]
fn test_preg_replace_callback_preserves_escaped_capture_free_match_arrays() {
    assert_eq!(
        run_php(
            r#"<?php
            function retain_first_match($matches) {
                static $first = null;
                if ($first === null) {
                    $first = $matches;
                    return $matches[0];
                }
                return $first[0] . ':' . $matches[0];
            }
            echo preg_replace_callback('/[a-z]+/', 'retain_first_match', 'a bb');
            "#,
        ),
        "a a:bb"
    );
}

#[test]
fn test_preg_replace_callback_capture_free_argument_keeps_cow_mutation() {
    assert_eq!(
        run_php(
            r#"<?php
            function mutate_match($matches) {
                $matches[0] = '<' . $matches[0] . '>';
                return $matches[0];
            }
            echo preg_replace_callback('/[a-z]+/', 'mutate_match', 'a bb');
            "#,
        ),
        "<a> <bb>"
    );
}

#[test]
fn test_preg_replace_callback_exception_does_not_publish_partial_output() {
    assert_eq!(
        run_php(
            r#"<?php
            function fail_on_second_match($matches) {
                if ($matches[0] === '2') {
                    throw new Exception('stop');
                }
                return '[' . $matches[0] . ']';
            }
            $result = 'unchanged';
            try {
                $result = preg_replace_callback('/\d/', 'fail_on_second_match', '123');
            } catch (Exception $error) {
                echo $result . '|' . $error->getMessage();
            }
            "#,
        ),
        "unchanged|stop"
    );
}

#[test]
fn test_preg_quote_escapes_regex_metacharacters_and_delimiter() {
    assert_eq!(
        run_php("<?php echo preg_quote('a.b/c+', '/');"),
        "a\\.b\\/c\\+"
    );
}
#[test]
fn test_preg_match_all_offset_capture_set_order() {
    assert_eq!(
        run_php(
            "<?php preg_match_all('/\\{(!)?(\\w+)\\}/', '/a/{id}/{!slug}', $matches, PREG_OFFSET_CAPTURE | PREG_SET_ORDER); foreach ($matches as $match) { echo $match[0][0] . ':' . $match[0][1] . ':' . $match[1][1] . ':' . $match[2][0] . '|'; }"
        ),
        "{id}:3:-1:id|{!slug}:8:9:slug|"
    );
}
