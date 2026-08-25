mod common;

use common::run_php;

#[test]
fn invalid_utf8_recovery_uses_maximal_subparts_and_ignore_precedence() {
    assert_eq!(
        run_php(
            r#"<?php
$inputs = [
    'continuations' => chr(0x80) . chr(0xBF) . 'A',
    'truncated' => chr(0xE2) . chr(0x82) . 'B',
    'surrogate' => chr(0xED) . chr(0xA0) . chr(0x80) . 'C',
    'too-high' => chr(0xF4) . chr(0x90) . chr(0x80) . chr(0x80) . 'D',
    'valid' => chr(0xF0) . chr(0x9F) . chr(0x98) . chr(0x80) . '<&',
];
$flagSets = [
    'reject' => ENT_QUOTES,
    'ignore' => ENT_QUOTES | ENT_IGNORE,
    'substitute' => ENT_QUOTES | ENT_SUBSTITUTE,
    'both' => ENT_QUOTES | ENT_IGNORE | ENT_SUBSTITUTE,
];
foreach ($inputs as $name => $input) {
    foreach ($flagSets as $flagName => $flags) {
        echo "$name/$flagName=";
        foreach (['htmlspecialchars', 'htmlentities'] as $function) {
            echo bin2hex($function($input, $flags, 'UTF-8')), '|';
        }
        echo "\n";
    }
}
"#,
        ),
        concat!(
            "continuations/reject=||\n",
            "continuations/ignore=41|41|\n",
            "continuations/substitute=efbfbdefbfbd41|efbfbdefbfbd41|\n",
            "continuations/both=41|41|\n",
            "truncated/reject=||\n",
            "truncated/ignore=42|42|\n",
            "truncated/substitute=efbfbd42|efbfbd42|\n",
            "truncated/both=42|42|\n",
            "surrogate/reject=||\n",
            "surrogate/ignore=43|43|\n",
            "surrogate/substitute=efbfbd43|efbfbd43|\n",
            "surrogate/both=43|43|\n",
            "too-high/reject=||\n",
            "too-high/ignore=44|44|\n",
            "too-high/substitute=efbfbd44|efbfbd44|\n",
            "too-high/both=44|44|\n",
            "valid/reject=f09f9880266c743b26616d703b|f09f9880266c743b26616d703b|\n",
            "valid/ignore=f09f9880266c743b26616d703b|f09f9880266c743b26616d703b|\n",
            "valid/substitute=f09f9880266c743b26616d703b|f09f9880266c743b26616d703b|\n",
            "valid/both=f09f9880266c743b26616d703b|f09f9880266c743b26616d703b|\n",
        )
    );
}

#[test]
fn byte_escaped_literals_remain_distinct_from_unicode_source_text() {
    assert_eq!(
        run_php(
            r#"<?php
echo 'ignore=', ENT_IGNORE, "\n";
echo 'literal-valid=', bin2hex(htmlentities("Â", ENT_QUOTES, 'UTF-8')), "\n";
echo 'escaped-valid=', bin2hex(htmlentities("\xC2\xA0", ENT_QUOTES, 'UTF-8')), "\n";
echo 'literal-invalid=', bin2hex(htmlentities("\xC2", ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8')), "\n";
$tail = 'Z';
echo 'interpolated=', bin2hex(htmlentities("\xE2\x82{$tail}", ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8')), "\n";
"#,
        ),
        concat!(
            "ignore=4\n",
            "literal-valid=2641636972633b\n",
            "escaped-valid=266e6273703b\n",
            "literal-invalid=efbfbd\n",
            "interpolated=efbfbd5a\n",
        )
    );
}

#[test]
fn windows_1252_named_entities_share_the_reversible_legacy_engine() {
    assert_eq!(
        run_php(
            r#"<?php
$source = chr(0x80) . chr(0x82) . chr(0x9F) . '<&';
foreach (['htmlspecialchars', 'htmlentities'] as $function) {
    echo "$function=", bin2hex($function($source, ENT_QUOTES, 'Windows-1252')), "\n";
}
"#,
        ),
        concat!(
            "htmlspecialchars=80829f266c743b26616d703b\n",
            "htmlentities=266575726f3b26736271756f3b2659756d6c3b266c743b26616d703b\n",
        )
    );
}
