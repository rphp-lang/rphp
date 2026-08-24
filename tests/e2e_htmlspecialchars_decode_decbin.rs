mod common;

use common::run_php;

#[test]
fn htmlspecialchars_decode_applies_quote_and_document_flags_in_one_pass() {
    assert_eq!(
        run_php(
            r#"<?php
$source = '&amp;lt;|&quot;|&#34;|&apos;|&#039;|&#x27;|&lt;|&#60;|&gt;|&#62;|&#65;';
foreach ([
    ['none', ENT_NOQUOTES],
    ['compat', ENT_COMPAT],
    ['quotes', ENT_QUOTES],
    ['html5', ENT_QUOTES | ENT_HTML5],
] as [$name, $flags]) {
    echo $name, '=', bin2hex(htmlspecialchars_decode($source, $flags)), "\n";
}
"#,
        ),
        concat!(
            "none=266c743b7c2671756f743b7c262333343b7c2661706f733b7c26233033393b7c26237832373b7c3c7c3c7c3e7c3e7c262336353b\n",
            "compat=266c743b7c227c227c2661706f733b7c26233033393b7c26237832373b7c3c7c3c7c3e7c3e7c262336353b\n",
            "quotes=266c743b7c227c227c2661706f733b7c277c277c3c7c3c7c3e7c3e7c262336353b\n",
            "html5=266c743b7c227c227c277c277c277c3c7c3c7c3e7c3e7c262336353b\n",
        )
    );
}

#[test]
fn htmlspecialchars_decode_preserves_every_byte_and_embedded_nul() {
    assert_eq!(
        run_php(
            r#"<?php
$bytes = pack('C*', ...range(0, 255));
$decoded = htmlspecialchars_decode($bytes, ENT_QUOTES | ENT_HTML5);
echo strlen($decoded), "\n", bin2hex($decoded), "\n";
echo bin2hex(htmlspecialchars_decode("|&amp;|\0|&#x3C;", ENT_QUOTES | ENT_HTML5)), "\n";
"#,
        ),
        concat!(
            "256\n",
            "000102030405060708090a0b0c0d0e0f",
            "101112131415161718191a1b1c1d1e1f",
            "202122232425262728292a2b2c2d2e2f",
            "303132333435363738393a3b3c3d3e3f",
            "404142434445464748494a4b4c4d4e4f",
            "505152535455565758595a5b5c5d5e5f",
            "606162636465666768696a6b6c6d6e6f",
            "707172737475767778797a7b7c7d7e7f",
            "808182838485868788898a8b8c8d8e8f",
            "909192939495969798999a9b9c9d9e9f",
            "a0a1a2a3a4a5a6a7a8a9aaabacadaeaf",
            "b0b1b2b3b4b5b6b7b8b9babbbcbdbebf",
            "c0c1c2c3c4c5c6c7c8c9cacbcccdcecf",
            "d0d1d2d3d4d5d6d7d8d9dadbdcdddedf",
            "e0e1e2e3e4e5e6e7e8e9eaebecedeeef",
            "f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff\n",
            "7c267c007c3c\n",
        )
    );
}

#[test]
fn decbin_uses_amd64_twos_complement_boundaries_and_named_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([0, 1, 2, 15, 16, 255, -1, -2, PHP_INT_MAX, PHP_INT_MIN] as $number) {
    echo $number, '=', decbin($number), "\n";
}
echo 'named=', decbin(num: 10), "\n";
"#,
        ),
        concat!(
            "0=0\n",
            "1=1\n",
            "2=10\n",
            "15=1111\n",
            "16=10000\n",
            "255=11111111\n",
            "-1=1111111111111111111111111111111111111111111111111111111111111111\n",
            "-2=1111111111111111111111111111111111111111111111111111111111111110\n",
            "9223372036854775807=111111111111111111111111111111111111111111111111111111111111111\n",
            "-9223372036854775808=1000000000000000000000000000000000000000000000000000000000000000\n",
            "named=1010\n",
        )
    );
}

#[test]
fn chr_and_ord_round_trip_byte_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo 'warning=', $severity, ':', $message, "\n";
    return true;
});
foreach ([0, 1, 127, 128, 255, 256, -1] as $codepoint) {
    $byte = chr($codepoint);
    echo $codepoint, '=', bin2hex($byte), ':', ord($byte), "\n";
}
"#,
        ),
        concat!(
            "0=00:0\n",
            "1=01:1\n",
            "127=7f:127\n",
            "128=80:128\n",
            "255=ff:255\n",
            "warning=8192:chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256\n",
            "256=00:0\n",
            "warning=8192:chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256\n",
            "-1=ff:255\n",
        )
    );
}

#[test]
fn decbin_weak_coercions_and_diagnostics_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo 'warning=', $severity, ':', $message, "\n";
    return true;
});
foreach ([true, false, null, 15.9, '16', '16.9'] as $value) {
    echo get_debug_type($value), '=', decbin($value), "\n";
}
try { decbin([]); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "bool=1\n",
            "bool=0\n",
            "null=warning=8192:decbin(): Passing null to parameter #1 ($num) of type int is deprecated\n",
            "0\n",
            "float=warning=8192:Implicit conversion from float 15.9 to int loses precision\n",
            "1111\n",
            "string=10000\n",
            "string=warning=8192:Implicit conversion from float-string \"16.9\" to int loses precision\n",
            "10000\n",
            "decbin(): Argument #1 ($num) must be of type int, array given\n",
        )
    );
}

#[test]
fn decode_and_decbin_respect_strict_scalar_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach ([true, 1.0, '1', null] as $value) {
    try { decbin($value); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
try { htmlspecialchars_decode(1); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { htmlspecialchars_decode('&amp;', flags: []); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { chr('65'); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "decbin(): Argument #1 ($num) must be of type int, true given\n",
            "decbin(): Argument #1 ($num) must be of type int, float given\n",
            "decbin(): Argument #1 ($num) must be of type int, string given\n",
            "decbin(): Argument #1 ($num) must be of type int, null given\n",
            "htmlspecialchars_decode(): Argument #1 ($string) must be of type string, int given\n",
            "htmlspecialchars_decode(): Argument #2 ($flags) must be of type int, array given\n",
            "chr(): Argument #1 ($codepoint) must be of type int, string given\n",
        )
    );
}
