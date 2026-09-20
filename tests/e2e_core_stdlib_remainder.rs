mod common;

use common::{run_php, run_php_expect_error_with_source_context};

#[test]
fn formatting_and_string_projection_preserve_php_bytes_and_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
echo number_format(0.25, 2, '', ''), "\n";
echo number_format(1234, 2, '', ','), "\n";
echo bin2hex(strval("\x80\xff")), "\n";
ini_set('default_charset', 'cp1252');
var_dump(htmlentities("\xA3", ENT_HTML5));
var_dump(bin2hex(html_entity_decode('&pound;', ENT_HTML5)));
"#,
        ),
        concat!(
            "025\n",
            "1,23400\n",
            "80ff\n",
            "string(7) \"&pound;\"\n",
            "string(2) \"a3\"\n",
        )
    );
}

#[test]
fn locale_introspection_and_collation_use_the_active_process_locale() {
    assert_eq!(
        run_php(
            r#"<?php
setlocale(LC_ALL, 'C');
var_dump(strcoll('a', 'A'));
foreach ([ABDAY_2, DAY_4, ABMON_7, MON_4, RADIXCHAR] as $item) {
    var_dump(nl_langinfo($item));
}
"#,
        ),
        concat!(
            "int(32)\n",
            "string(3) \"Mon\"\n",
            "string(9) \"Wednesday\"\n",
            "string(3) \"Jul\"\n",
            "string(5) \"April\"\n",
            "string(1) \".\"\n",
        )
    );
}

#[test]
fn resource_and_extension_introspection_expose_live_request_state() {
    assert_eq!(
        run_php(
            r#"<?php
fclose(fopen('php://temp', 'w+'));
$count = count(get_resources());
fclose(fopen('php://temp', 'w+'));
var_dump(count(get_resources()) === $count);
var_dump(is_array(get_extension_funcs('standard')));
var_dump(is_array(get_extension_funcs('zend')));
var_dump(get_extension_funcs('missing'));
var_dump(is_string(PHP_BINARY), PHP_BINARY !== '');
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
            "bool(true)\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn system_streams_stdout_and_returns_the_last_line_and_status() {
    assert_eq!(
        run_php(
            r#"<?php
ob_start();
$last = system("printf 'one\\ntwo\\n'", $status);
$all = ob_get_clean();
var_dump($all, $last, $status);
"#,
        ),
        concat!(
            "string(8) \"one\n",
            "two\n",
            "\"\n",
            "string(3) \"two\"\n",
            "int(0)\n",
        )
    );
}

#[test]
fn oversized_padding_uses_the_canonical_allocation_fatal() {
    let error = run_php_expect_error_with_source_context(
        "<?php str_pad('x', PHP_INT_MAX);",
        "/tmp/padding.php",
        "/tmp",
    );
    let message = error.to_string();
    assert!(message.starts_with(
        "Allowed memory size of 134217728 bytes exhausted (tried to allocate 9223372036854775807 bytes)"
    ));
    assert!(message.ends_with("in /tmp/padding.php on line 1"));
}
