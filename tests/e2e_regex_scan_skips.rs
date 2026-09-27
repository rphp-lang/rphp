mod common;

use common::run_php;

/// The scanning loops of preg_replace/preg_split/preg_match_all/preg_match
/// stop after position 0 for subject-anchored patterns and jump to the next
/// occurrence of a required first literal. Anchors under the multiline flag,
/// `\G`, the `A` modifier, empty matches, alternations, groups, marks,
/// lookaheads and UTF-8 subjects must keep PHP's results. Verified against
/// reference PHP.
#[test]
fn anchored_and_literal_prefixed_scans_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
$s = "foo bar\nfoo baz\nqux foo";
$cases = [
  ['/^foo/', 'X'], ['/^foo/m', 'X'], ['/\Afoo/', 'X'], ['/\Afoo/m', 'X'], ['/^foo|^qux/', 'X'], ['/^foo|qux/', 'X'],
  ['/(^foo)/', 'X'], ['/(?:^foo)+/', 'X'], ['/^/', 'X'], ['/^/m', 'X'], ['/^$/m', 'X'], ['/\Gfoo/', 'X'],
  ['/foo/A', 'X'], ['/o/', '0'], ['/O/i', '0'], ['/(?i)o/', '0'], ['/ba./', '_'], ['/x*/', '-'], ['/z?/', '-'],
  ['/(*MARK:a)^foo/', 'X'], ['/^\s*/m', '>'], ['/(?=foo)/', '|'], ['/\bfoo\b/', 'F'], ['/^(?<w>\w+)/', '[$1]'],
];
foreach ($cases as [$p, $r]) {
    $out = preg_replace($p, $r, $s, -1, $n);
    echo json_encode($out), ' ', $n, ' ', json_encode(preg_split($p, $s)), ' ', preg_match_all($p, $s, $m), ' ', json_encode($m[0]), ' ', json_encode(preg_match($p, $s, $mm)), json_encode($mm), "\n";
}
echo json_encode(preg_replace_callback('/^foo/', fn($m) => strtoupper($m[0]), $s)), json_encode(preg_replace_callback('/^foo/m', fn($m) => strtoupper($m[0]), $s)), "\n";
echo json_encode(preg_replace('/^a/', 'X', ['abc', 'bac', 'aaa'])), json_encode(preg_replace(['/^a/', '/c$/'], ['X', 'Y'], 'abcabc')), "\n";
echo json_encode(preg_replace('/^é/u', 'E', "élan\nébène")), json_encode(preg_replace('/^é/mu', 'E', "élan\nébène")), json_encode(preg_replace('/é/u', 'E', "élan\nébène")), "\n";
"#
        ),
        "\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1[\"foo\"]\n\"X bar\\nX baz\\nqux foo\" 2 [\"\",\" bar\\n\",\" baz\\nqux foo\"] 2 [\"foo\",\"foo\"] 1[\"foo\"]\n\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1[\"foo\"]\n\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1[\"foo\"]\n\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1[\"foo\"]\n\"X bar\\nfoo baz\\nX foo\" 2 [\"\",\" bar\\nfoo baz\\n\",\" foo\"] 2 [\"foo\",\"qux\"] 1[\"foo\"]\n\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1[\"foo\",\"foo\"]\n\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1[\"foo\"]\n\"Xfoo bar\\nfoo baz\\nqux foo\" 1 [\"\",\"foo bar\\nfoo baz\\nqux foo\"] 1 [\"\"] 1[\"\"]\n\"Xfoo bar\\nXfoo baz\\nXqux foo\" 3 [\"\",\"foo bar\\n\",\"foo baz\\n\",\"qux foo\"] 3 [\"\",\"\",\"\"] 1[\"\"]\n\"foo bar\\nfoo baz\\nqux foo\" 0 [\"foo bar\\nfoo baz\\nqux foo\"] 0 [] 0[]\n\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1[\"foo\"]\n\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1[\"foo\"]\n\"f00 bar\\nf00 baz\\nqux f00\" 6 [\"f\",\"\",\" bar\\nf\",\"\",\" baz\\nqux f\",\"\",\"\"] 6 [\"o\",\"o\",\"o\",\"o\",\"o\",\"o\"] 1[\"o\"]\n\"f00 bar\\nf00 baz\\nqux f00\" 6 [\"f\",\"\",\" bar\\nf\",\"\",\" baz\\nqux f\",\"\",\"\"] 6 [\"o\",\"o\",\"o\",\"o\",\"o\",\"o\"] 1[\"o\"]\n\"f00 bar\\nf00 baz\\nqux f00\" 6 [\"f\",\"\",\" bar\\nf\",\"\",\" baz\\nqux f\",\"\",\"\"] 6 [\"o\",\"o\",\"o\",\"o\",\"o\",\"o\"] 1[\"o\"]\n\"foo _\\nfoo _\\nqux foo\" 2 [\"foo \",\"\\nfoo \",\"\\nqux foo\"] 2 [\"bar\",\"baz\"] 1[\"bar\"]\n\"-f-o-o- -b-a-r-\\n-f-o-o- -b-a-z-\\n-q-u-- -f-o-o-\" 24 [\"\",\"f\",\"o\",\"o\",\" \",\"b\",\"a\",\"r\",\"\\n\",\"f\",\"o\",\"o\",\" \",\"b\",\"a\",\"z\",\"\\n\",\"q\",\"u\",\"\",\" \",\"f\",\"o\",\"o\",\"\"] 24 [\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"x\",\"\",\"\",\"\",\"\",\"\"] 1[\"\"]\n\"-f-o-o- -b-a-r-\\n-f-o-o- -b-a--\\n-q-u-x- -f-o-o-\" 24 [\"\",\"f\",\"o\",\"o\",\" \",\"b\",\"a\",\"r\",\"\\n\",\"f\",\"o\",\"o\",\" \",\"b\",\"a\",\"\",\"\\n\",\"q\",\"u\",\"x\",\" \",\"f\",\"o\",\"o\",\"\"] 24 [\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"z\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\",\"\"] 1[\"\"]\n\"X bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1{\"0\":\"foo\",\"MARK\":\"a\"}\n\">foo bar\\n>foo baz\\n>qux foo\" 3 [\"\",\"foo bar\\n\",\"foo baz\\n\",\"qux foo\"] 3 [\"\",\"\",\"\"] 1[\"\"]\n\"|foo bar\\n|foo baz\\nqux |foo\" 3 [\"\",\"foo bar\\n\",\"foo baz\\nqux \",\"foo\"] 3 [\"\",\"\",\"\"] 1[\"\"]\n\"F bar\\nF baz\\nqux F\" 3 [\"\",\" bar\\n\",\" baz\\nqux \",\"\"] 3 [\"foo\",\"foo\",\"foo\"] 1[\"foo\"]\n\"[foo] bar\\nfoo baz\\nqux foo\" 1 [\"\",\" bar\\nfoo baz\\nqux foo\"] 1 [\"foo\"] 1{\"0\":\"foo\",\"w\":\"foo\",\"1\":\"foo\"}\n\"FOO bar\\nfoo baz\\nqux foo\"\"FOO bar\\nFOO baz\\nqux foo\"\n[\"Xbc\",\"bac\",\"Xaa\"]\"XbcabY\"\n\"Elan\\n\\u00e9b\\u00e8ne\"\"Elan\\nEb\\u00e8ne\"\"Elan\\nEb\\u00e8ne\"\n"
    );
}
