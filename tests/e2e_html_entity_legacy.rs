mod common;

use common::run_php;

#[test]
fn numeric_entities_map_to_php_85_legacy_bytes_and_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    ['ISO-8859-1', '&#233;'],
    ['iso8859-15', '&#x20AC;'],
    ['ISO8859-5', '&#x410;'],
    ['866', '&#x410;'],
    ['koi8-ru', '&#x410;'],
    ['MacRoman', '&#x20AC;'],
    ['win-1251', '&#x20AC;'],
    ['1252', '&#x20AC;'],
];
foreach ($cases as [$encoding, $entity]) {
    echo $encoding, '=', bin2hex(html_entity_decode($entity, ENT_QUOTES, $encoding)), "\n";
}
"#,
        ),
        concat!(
            "ISO-8859-1=e9\n",
            "iso8859-15=a4\n",
            "ISO8859-5=b0\n",
            "866=80\n",
            "koi8-ru=e1\n",
            "MacRoman=db\n",
            "win-1251=88\n",
            "1252=80\n",
        )
    );
}

#[test]
fn invalid_control_undefined_and_unrepresentable_entities_remain_verbatim() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    ['Windows-1252', '&#x81;'],
    ['Windows-1251', '&#x98;'],
    ['CP866', '&#x80;'],
    ['ISO-8859-5', '&#x80;'],
    ['MacRoman', '&#x7F;'],
    ['ISO-8859-1', '&#x20AC;'],
    ['ISO-8859-15', '&#xA4;'],
    ['KOI8-R', '&#x20AC;'],
    ['UTF-8', '&#xD800;'],
    ['UTF-8', '&#x110000;'],
];
foreach ($cases as [$encoding, $entity]) {
    echo $encoding, ':', $entity, '=',
        bin2hex(html_entity_decode($entity, ENT_QUOTES, $encoding)), "\n";
}
"#,
        ),
        concat!(
            "Windows-1252:&#x81;=26237838313b\n",
            "Windows-1251:&#x98;=26237839383b\n",
            "CP866:&#x80;=26237838303b\n",
            "ISO-8859-5:&#x80;=26237838303b\n",
            "MacRoman:&#x7F;=26237837463b\n",
            "ISO-8859-1:&#x20AC;=262378323041433b\n",
            "ISO-8859-15:&#xA4;=26237841343b\n",
            "KOI8-R:&#x20AC;=262378323041433b\n",
            "UTF-8:&#xD800;=262378443830303b\n",
            "UTF-8:&#x110000;=2623783131303030303b\n",
        )
    );
}

#[test]
fn valid_entities_after_invalid_prefixes_are_still_decoded() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['&&amp;', '&#&amp;', '&bogus;&copy;', '&#x1&amp;', '&aa;&#x24;'] as $source) {
    echo bin2hex(html_entity_decode($source, ENT_QUOTES, 'UTF-8')), "\n";
}
"#,
        ),
        concat!(
            "2626\n",
            "262326\n",
            "26626f6775733bc2a9\n",
            "2623783126\n",
            "2661613b24\n",
        )
    );
}

#[test]
fn numeric_parser_accepts_long_zero_padding_and_preserves_malformed_forms() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    '&#'.str_repeat('0', 80).'65;',
    '&#x'.str_repeat('0', 80).'41;',
    '&#;',
    '&#x;',
    '&#12z;',
    '&#65',
    '&#4294967296;',
    '&#1114111;',
    '&#1114112;',
];
foreach ($cases as $source) {
    echo bin2hex(html_entity_decode($source, ENT_QUOTES, 'UTF-8')), "\n";
}
"#,
        ),
        concat!(
            "41\n",
            "41\n",
            "26233b\n",
            "2623783b\n",
            "262331327a3b\n",
            "26233635\n",
            "2623343239343936373239363b\n",
            "f48fbfbf\n",
            "2623313131343131323b\n",
        )
    );
}

#[test]
fn quote_flags_and_document_validity_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([ENT_NOQUOTES, ENT_COMPAT, ENT_QUOTES] as $flags) {
    echo 'q', $flags, '=', bin2hex(html_entity_decode(
        '&quot;|&#34;|&#39;|&apos;', $flags, 'UTF-8'
    )), "\n";
}
$documents = [
    ['html401', ENT_QUOTES],
    ['xml1', ENT_QUOTES | ENT_XML1],
    ['xhtml', ENT_QUOTES | ENT_XHTML],
    ['html5', ENT_QUOTES | ENT_HTML5],
];
foreach ($documents as [$name, $flags]) {
    echo $name, '=', bin2hex(html_entity_decode(
        '&#9;|&#11;|&#12;|&#13;|&#127;|&#128;|&#159;|&#160;|&#xFDD0;|&#xFFFE;|&#x10000;|&apos;',
        $flags,
        'UTF-8'
    )), "\n";
}
"#,
        ),
        concat!(
            "q0=2671756f743b7c262333343b7c262333393b7c2661706f733b\n",
            "q2=227c227c262333393b7c2661706f733b\n",
            "q3=227c227c277c2661706f733b\n",
            "html401=097c262331313b7c262331323b7c0d7c26233132373b7c26233132383b7c26233135393b7cc2a07cefb7907cefbfbe7cf09080807c2661706f733b\n",
            "xml1=097c262331313b7c262331323b7c0d7c7f7cc2807cc29f7cc2a07cefb7907c262378464646453b7cf09080807c27\n",
            "xhtml=097c262331313b7c262331323b7c0d7c7f7cc2807cc29f7cc2a07cefb7907c262378464646453b7cf09080807c27\n",
            "html5=097c262331313b7c0c7c262331333b7c26233132373b7c26233132383b7c26233135393b7cc2a07c262378464444303b7c262378464646453b7cf09080807c27\n",
        )
    );
}

#[test]
fn binary_boundaries_named_entities_and_charset_warning_are_preserved() {
    assert_eq!(
        run_php(
            r#"<?php
$source = "A\0&#x20AC;Z";
$decoded = html_entity_decode($source, ENT_QUOTES, '1252');
echo 'adjacent=', bin2hex($decoded), ':', strlen($decoded), "\n";
foreach ([
    ['CP866', '&nbsp;|&#160;|&copy;|&#169;'],
    ['KOI8-R', '&copy;|&#169;|&nbsp;|&#160;'],
] as [$encoding, $entities]) {
    echo $encoding, '=', bin2hex(html_entity_decode($entities, ENT_QUOTES, $encoding)), "\n";
}
set_error_handler(function ($severity, $message) {
    echo 'warning=', $severity, ':', $message, "\n";
    return true;
});
echo 'fallback=', bin2hex(html_entity_decode('&#233;', ENT_QUOTES, 'utf8')), "\n";
"#,
        ),
        concat!(
            "adjacent=4100805a:4\n",
            "CP866=ff7cff7c26636f70793b7c26233136393b\n",
            "KOI8-R=bf7cbf7c9a7c9a\n",
            "fallback=warning=2:html_entity_decode(): Charset \"utf8\" is not supported, assuming UTF-8\n",
            "c3a9\n",
        )
    );
}
