mod common;

use common::run_php;

#[test]
fn translation_tables_apply_document_quote_and_canonical_alias_rules() {
    assert_eq!(
        run_php(
            r#"<?php
$documents = [
    ['h4', ENT_HTML401],
    ['xml', ENT_XML1],
    ['xhtml', ENT_XHTML],
    ['h5', ENT_HTML5],
];
foreach ($documents as [$name, $document]) {
    foreach ([HTML_SPECIALCHARS, HTML_ENTITIES] as $table) {
        $map = get_html_translation_table($table, ENT_QUOTES | $document, 'UTF-8');
        echo $name, '/', $table, '=', count($map), ':',
            $map["'"] ?? '-', ':', $map['∵'] ?? '-', "\n";
    }
}
"#,
        ),
        concat!(
            "h4/0=5:&#039;:-\n",
            "h4/1=253:&#039;:-\n",
            "xml/0=5:&apos;:-\n",
            "xml/1=5:&apos;:-\n",
            "xhtml/0=5:&apos;:-\n",
            "xhtml/1=253:&#039;:-\n",
            "h5/0=5:&apos;:-\n",
            "h5/1=1511:&apos;:&Because;\n",
        )
    );
}

#[test]
fn legacy_tables_filter_and_sort_external_byte_keys() {
    assert_eq!(
        run_php(
            r#"<?php
function raw_key_compare($left, $right) {
    return ord($left[0]) - ord($right[0]);
}
foreach (['WINDOWS-1252', 'Windows-1251', 'ISO-8859-1', 'SJIS'] as $encoding) {
    $map = get_html_translation_table(HTML_ENTITIES, ENT_QUOTES | ENT_HTML5, $encoding);
    uksort($map, 'raw_key_compare');
    $keys = array_keys($map);
    echo $encoding, '=', count($map), ':', bin2hex($keys[0]), ':',
        bin2hex($keys[count($keys) - 1]), "\n";
}
$map = get_html_translation_table(HTML_ENTITIES, ENT_QUOTES | ENT_HTML5, 'Windows-1252');
krsort($map);
$rendered = print_r($map, true);
$firstKeyOffset = strpos($rendered, '[') + 1;
echo 'krsort-rendered-first=', bin2hex($rendered[$firstKeyOffset]), "\n";
foreach ($map as $key => $entity) {
    echo 'foreach-first=', bin2hex($key), ':', $entity, "\n";
    break;
}
"#,
        ),
        concat!(
            "WINDOWS-1252=156:09:ff\n",
            "Windows-1251=158:09:ff\n",
            "ISO-8859-1=132:09:ff\n",
            "SJIS=5:22:3e\n",
            "krsort-rendered-first=ff\n",
            "foreach-first=ff:&yuml;\n",
        )
    );
}

#[test]
fn defaults_named_arguments_and_diagnostics_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
echo 'defaults=', json_encode(get_html_translation_table()), "\n";
echo 'named=', count(get_html_translation_table(
    table: HTML_ENTITIES,
    flags: ENT_COMPAT | ENT_HTML5,
    encoding: 'UTF-8'
)), "\n";
set_error_handler(function ($severity, $message) {
    echo 'warning=', $severity, ':', $message, "\n";
    return true;
});
echo 'fallback=', count(get_html_translation_table(encoding: 'utf8')), "\n";
restore_error_handler();
foreach ([
    ['table', fn() => get_html_translation_table(table: [])],
    ['flags', fn() => get_html_translation_table(flags: [])],
    ['encoding', fn() => get_html_translation_table(encoding: [])],
] as [$name, $call]) {
    try { $call(); }
    catch (Throwable $error) { echo $name, '=', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "defaults={\"\\\"\":\"&quot;\",\"&\":\"&amp;\",\"'\":\"&#039;\",\"<\":\"&lt;\",\">\":\"&gt;\"}\n",
            "named=1510\n",
            "fallback=warning=2:get_html_translation_table(): Charset \"utf8\" is not supported, assuming UTF-8\n",
            "5\n",
            "table=get_html_translation_table(): Argument #1 ($table) must be of type int, array given\n",
            "flags=get_html_translation_table(): Argument #2 ($flags) must be of type int, array given\n",
            "encoding=get_html_translation_table(): Argument #3 ($encoding) must be of type string, array given\n",
        )
    );
}
