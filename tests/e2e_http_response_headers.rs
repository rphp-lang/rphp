mod common;
use common::run_php;

#[test]
fn http_response_header_state_and_reflection_match_php_85() {
    let source = r#"<?php
foreach (['http_get_last_response_headers', 'http_clear_last_response_headers'] as $name) {
    $reflection = new ReflectionFunction($name);
    var_dump(
        $reflection->getExtensionName(),
        (string) $reflection->getReturnType(),
        $reflection->getNumberOfParameters(),
        $reflection->getNumberOfRequiredParameters()
    );
}
var_dump(
    http_get_last_response_headers(),
    http_clear_last_response_headers(),
    http_get_last_response_headers()
);
"#;

    assert_eq!(
        run_php(source),
        "string(8) \"standard\"\nstring(6) \"?array\"\nint(0)\nint(0)\nstring(8) \"standard\"\nstring(4) \"void\"\nint(0)\nint(0)\nNULL\nNULL\nNULL\n"
    );
}
