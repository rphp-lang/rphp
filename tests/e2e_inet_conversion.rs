mod common;

use common::run_php;

#[test]
fn inet_binary_address_conversion_matches_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['inet_pton', 'inet_ntop'] as $name) {
    $function = new ReflectionFunction($name);
    echo $function->getExtensionName(), ':', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', $function->getReturnType(), '|';
}
echo bin2hex(inet_pton('127.0.0.1')), ':', inet_ntop("\x7f\0\0\1"), '|';
echo bin2hex(inet_pton('2001:db8::1')), ':', inet_ntop(hex2bin('20010db8000000000000000000000001')), '|';
var_export(inet_pton('not-an-address'));
echo ':';
var_export(inet_ntop('short'));
"#,
        ),
        concat!(
            "standard:1/1:string|false|standard:1/1:string|false|",
            "7f000001:127.0.0.1|20010db8000000000000000000000001:2001:db8::1|false:false",
        ),
    );
}
