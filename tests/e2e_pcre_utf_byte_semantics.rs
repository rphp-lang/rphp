mod common;

use common::run_php;

#[test]
fn utf_mode_matches_codepoints_and_reports_byte_offsets() {
    assert_eq!(
        run_php(
            r#"<?php
$subject = "\xe2\x82\xacA";
preg_match_all('/./u', $subject, $matches, PREG_OFFSET_CAPTURE);
foreach ($matches[0] as $match) echo bin2hex($match[0]), ':', $match[1], '|';
foreach (preg_split('//u', $subject, -1, PREG_SPLIT_NO_EMPTY) as $part) echo bin2hex($part), '|';
echo preg_match('/\w/u', 'ä'), '|';
preg_match('/\bwasser/iu', 'Süßwasser Wasser', $word, PREG_OFFSET_CAPTURE);
echo $word[0][0], ':', $word[0][1];
"#,
        ),
        "e282ac:0|41:3|e282ac|41|1|Wasser:12"
    );
}

#[test]
fn utf_mode_distinguishes_malformed_subjects_from_bad_offsets() {
    assert_eq!(
        run_php(
            r#"<?php
$matches = ['stale'];
var_dump(preg_match('/./u', "\xff", $matches), $matches, preg_last_error());
var_dump(preg_match('/./u', "\xc3\xa9", $matches, 0, 1), $matches, preg_last_error());
var_dump(preg_match('/./u', "\xc3\xa9", $matches, 0, 2), $matches, preg_last_error());
$suffix = "VA\xffLID";
var_dump(preg_match('/\b/u', $suffix, $matches, 0, 4), preg_last_error());
var_dump(preg_match('/\b/u', $suffix, $matches, 0, 0), $matches, preg_last_error());
"#,
        ),
        concat!(
            "bool(false)\narray(0) {\n}\nint(4)\n",
            "bool(false)\narray(0) {\n}\nint(5)\n",
            "int(0)\narray(0) {\n}\nint(0)\n",
            "int(1)\nint(0)\n",
            "bool(false)\narray(0) {\n}\nint(4)\n",
        )
    );
}

#[test]
fn utf_mode_errors_propagate_across_split_replace_and_grep() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(preg_split('/a/u', "a\xff"), preg_last_error(), preg_last_error_msg());
var_dump(preg_replace('/./u', 'x', "a\xff"), preg_last_error());
var_dump(preg_grep('/\d/u', ['x', '1', "2\xff", '3']), preg_last_error());
"#,
        ),
        concat!(
            "bool(false)\nint(4)\nstring(56) \"Malformed UTF-8 characters, possibly incorrectly encoded\"\n",
            "NULL\nint(4)\n",
            "array(1) {\n  [1]=>\n  string(1) \"1\"\n}\nint(4)\n",
        )
    );
}

#[test]
fn preg_match_all_clears_each_declared_capture_on_utf_error() {
    assert_eq!(
        run_php(
            r#"<?php
$matches = ['stale'];
var_dump(preg_match_all('/(?<letter>.)/u', "a\xff", $matches), $matches, preg_last_error());
"#,
        ),
        concat!(
            "bool(false)\narray(3) {\n",
            "  [0]=>\n  array(0) {\n  }\n",
            "  [\"letter\"]=>\n  array(0) {\n  }\n",
            "  [1]=>\n  array(0) {\n  }\n",
            "}\nint(4)\n",
        )
    );
}

#[test]
fn utf_mode_unicode_shorthands_do_not_change_plain_byte_mode() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['ä', "\u{2003}"] as $value) {
    echo preg_match('/\w/u', $value), preg_match('/\s/u', $value), '|';
}
echo preg_match('/\w/', 'ä'), preg_match('/\s/', "\u{2003}");
"#,
        ),
        "10|01|00"
    );
}
