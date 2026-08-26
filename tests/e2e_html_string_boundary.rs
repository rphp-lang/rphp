mod common;

use common::run_php;

#[test]
fn html_encoders_resolve_charset_labels_and_preserve_php_bytes() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo "diag=", $severity, ":", $message, "\n";
});
$input = pack('C*', 0x3c, 0x26, 0x3e, 0, 0x80);
foreach ([1, 1252, 8666, null, '', "bogus\0tail", "UTF-8\0ignored"] as $encoding) {
    foreach (['htmlspecialchars', 'htmlentities'] as $function) {
        echo $function, '/', get_debug_type($encoding), '/', bin2hex((string) $encoding), '=';
        echo bin2hex($function(
            $input,
            ENT_QUOTES | ENT_SUBSTITUTE,
            $encoding,
            false
        )), "\n";
    }
}
echo 'table=', count(get_html_translation_table(encoding: null)), "\n";
"#,
        ),
        concat!(
            "htmlspecialchars/int/31=diag=2:htmlspecialchars(): Charset \"1\" is not supported, assuming UTF-8\n",
            "266c743b26616d703b2667743b00efbfbd\n",
            "htmlentities/int/31=diag=2:htmlentities(): Charset \"1\" is not supported, assuming UTF-8\n",
            "266c743b26616d703b2667743b00efbfbd\n",
            "htmlspecialchars/int/31323532=266c743b26616d703b2667743b0080\n",
            "htmlentities/int/31323532=266c743b26616d703b2667743b00266575726f3b\n",
            "htmlspecialchars/int/38363636=diag=2:htmlspecialchars(): Charset \"8666\" is not supported, assuming UTF-8\n",
            "266c743b26616d703b2667743b00efbfbd\n",
            "htmlentities/int/38363636=diag=2:htmlentities(): Charset \"8666\" is not supported, assuming UTF-8\n",
            "266c743b26616d703b2667743b00efbfbd\n",
            "htmlspecialchars/null/=266c743b26616d703b2667743b00efbfbd\n",
            "htmlentities/null/=266c743b26616d703b2667743b00efbfbd\n",
            "htmlspecialchars/string/=266c743b26616d703b2667743b00efbfbd\n",
            "htmlentities/string/=266c743b26616d703b2667743b00efbfbd\n",
            "htmlspecialchars/string/626f677573007461696c=diag=2:htmlspecialchars(): Charset \"bogus\" is not supported, assuming UTF-8\n",
            "266c743b26616d703b2667743b00efbfbd\n",
            "htmlentities/string/626f677573007461696c=diag=2:htmlentities(): Charset \"bogus\" is not supported, assuming UTF-8\n",
            "266c743b26616d703b2667743b00efbfbd\n",
            "htmlspecialchars/string/5554462d380069676e6f726564=266c743b26616d703b2667743b00efbfbd\n",
            "htmlentities/string/5554462d380069676e6f726564=266c743b26616d703b2667743b00efbfbd\n",
            "table=diag=8192:get_html_translation_table(): Passing null to parameter #3 ($encoding) of type string is deprecated\n",
            "5\n",
        )
    );
}

#[test]
fn double_encode_false_scans_long_numeric_entities_without_losing_cow() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([0, 32, 128, 4096] as $zeros) {
    foreach (['&#', '&#x'] as $prefix) {
        $entity = $prefix . str_repeat('0', $zeros) . '5;';
        foreach (['htmlspecialchars', 'htmlentities'] as $function) {
            $output = $function('<&' . $entity . '>', ENT_QUOTES, 'UTF-8', false);
            echo $function, '/', $zeros, '/', $prefix, '/', strlen($output), '/';
            echo $output === '&lt;&amp;' . $entity . '&gt;' ? "same\n" : "bad\n";
        }
    }
}
foreach ([
    '&#x110000;',
    '&#999999999999999999999999;',
    '&#xFFFFFFFFFFFFFFFF;',
    '&' . str_repeat('a', 100) . ';',
] as $entity) {
    echo bin2hex(htmlspecialchars($entity, ENT_QUOTES, 'UTF-8', false)), "\n";
}
$source = pack('C*', 0x3c, 0x26, 0x3e, 0, 0x80)
    . '&#' . str_repeat('0', 80) . '5;';
$alias =& $source;
$copy = $source;
$output = htmlspecialchars($alias, ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8', false);
echo strlen($output), '/', md5($output), '/';
echo $source === $copy ? 'cow/' : 'changed/';
echo $alias === $source ? "ref\n" : "split\n";
"#,
        ),
        concat!(
            "htmlspecialchars/0/&#/17/same\n",
            "htmlentities/0/&#/17/same\n",
            "htmlspecialchars/0/&#x/18/same\n",
            "htmlentities/0/&#x/18/same\n",
            "htmlspecialchars/32/&#/49/same\n",
            "htmlentities/32/&#/49/same\n",
            "htmlspecialchars/32/&#x/50/same\n",
            "htmlentities/32/&#x/50/same\n",
            "htmlspecialchars/128/&#/145/same\n",
            "htmlentities/128/&#/145/same\n",
            "htmlspecialchars/128/&#x/146/same\n",
            "htmlentities/128/&#x/146/same\n",
            "htmlspecialchars/4096/&#/4113/same\n",
            "htmlentities/4096/&#/4113/same\n",
            "htmlspecialchars/4096/&#x/4114/same\n",
            "htmlentities/4096/&#x/4114/same\n",
            "26616d703b23783131303030303b\n",
            "26616d703b233939393939393939393939393939393939393939393939393b\n",
            "26616d703b2378464646464646464646464646464646463b\n",
            "26616d703b616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161613b\n",
            "101/4b2af5fc01a5e4f4aa824dd039e031d4/cow/ref\n",
        )
    );
}

#[test]
fn html_encoder_call_shapes_snapshot_diagnostics_and_throwing_handlers() {
    assert_eq!(
        run_php(
            r#"<?php
class EncodingName {
    public function __construct(private string $value) {}
    public function __toString(): string {
        echo 'stringable=', $this->value, "\n";
        return $this->value;
    }
}
$function = 'htmlspecialchars';
echo 'dynamic=', $function('<&>', double_encode: false), "\n";
echo 'callback=', call_user_func('htmlspecialchars', '<&>', ENT_QUOTES, 'UTF-8', false), "\n";
$first = htmlspecialchars(...);
echo 'first=', $first(
    string: '<&>',
    flags: ENT_QUOTES,
    encoding: 'UTF-8',
    double_encode: false
), "\n";
set_error_handler(function ($severity, $message) {
    echo 'diag=', $severity, ':', $message, "\n";
});
echo 'weak=', htmlspecialchars(true, '11', new EncodingName('bogus'), 0), "\n";
try { htmlspecialchars('x', 11, 'bogus', []); }
catch (Throwable $error) { echo 'precedence=', $error->getMessage(), "\n"; }
restore_error_handler();
$source = '<&>';
$encoding = 'bogus';
set_error_handler(function ($severity, $message) use (&$source, &$encoding) {
    echo 'handler=', $message, "\n";
    $source = 'changed';
    $encoding = 'UTF-8';
});
echo 'snapshot=', htmlspecialchars($source, ENT_QUOTES, $encoding, false),
    '/', $source, '/', $encoding, "\n";
restore_error_handler();
set_error_handler(function ($severity, $message) {
    throw new Exception('warning-stop');
});
try { htmlentities('<&>', ENT_QUOTES, 'unknown', false); }
catch (Throwable $error) { echo 'throw=', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "dynamic=&lt;&amp;&gt;\n",
            "callback=&lt;&amp;&gt;\n",
            "first=&lt;&amp;&gt;\n",
            "weak=stringable=bogus\n",
            "diag=2:htmlspecialchars(): Charset \"bogus\" is not supported, assuming UTF-8\n",
            "1\n",
            "precedence=htmlspecialchars(): Argument #4 ($double_encode) must be of type bool, array given\n",
            "snapshot=handler=htmlspecialchars(): Charset \"bogus\" is not supported, assuming UTF-8\n",
            "&lt;&amp;&gt;/changed/UTF-8\n",
            "throw=warning-stop\n",
        )
    );
}

#[test]
fn html_function_reflection_and_strict_types_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'htmlspecialchars',
    'htmlentities',
    'htmlspecialchars_decode',
    'html_entity_decode',
    'get_html_translation_table',
] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, '/', (string) $reflection->getReturnType(), '/';
    echo str_contains((string) $reflection, '<internal:standard>') ? "standard\n" : "bad\n";
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), ':', (string) $parameter->getType(), ':';
        echo $parameter->isOptional() ? 'optional:' : 'required:';
        echo $parameter->isDefaultValueAvailable()
            ? json_encode($parameter->getDefaultValue()) . "\n"
            : "-\n";
    }
    echo 'defaults=';
    echo str_contains(
        (string) $reflection,
        'ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401'
    ) ? 'flags:' : 'no-flags:';
    echo str_contains((string) $reflection, 'HTML_SPECIALCHARS')
        ? "table\n"
        : "no-table\n";
}
"#,
        ),
        concat!(
            "htmlspecialchars/string/standard\n",
            "string:string:required:-\n",
            "flags:int:optional:11\n",
            "encoding:?string:optional:null\n",
            "double_encode:bool:optional:true\n",
            "defaults=flags:no-table\n",
            "htmlentities/string/standard\n",
            "string:string:required:-\n",
            "flags:int:optional:11\n",
            "encoding:?string:optional:null\n",
            "double_encode:bool:optional:true\n",
            "defaults=flags:no-table\n",
            "htmlspecialchars_decode/string/standard\n",
            "string:string:required:-\n",
            "flags:int:optional:11\n",
            "defaults=flags:no-table\n",
            "html_entity_decode/string/standard\n",
            "string:string:required:-\n",
            "flags:int:optional:11\n",
            "encoding:?string:optional:null\n",
            "defaults=flags:no-table\n",
            "get_html_translation_table/array/standard\n",
            "table:int:optional:0\n",
            "flags:int:optional:11\n",
            "encoding:string:optional:\"UTF-8\"\n",
            "defaults=flags:table\n",
        )
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class StrictStringable {
    public function __toString(): string { return 'x'; }
}
$dynamic = 'htmlspecialchars';
try { $dynamic(123); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { call_user_func('htmlspecialchars', 123); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
$first = htmlspecialchars(...);
try { $first(123); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { htmlspecialchars('x', '11', null, true); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { htmlspecialchars('x', 11, new StrictStringable(), true); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { htmlspecialchars('x', 11, null, 1); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { htmlentities(new StrictStringable(), 11, null, true); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
try { get_html_translation_table(0, 11, null); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "htmlspecialchars(): Argument #1 ($string) must be of type string, int given\n",
            "htmlspecialchars(): Argument #1 ($string) must be of type string, int given\n",
            "htmlspecialchars(): Argument #1 ($string) must be of type string, int given\n",
            "htmlspecialchars(): Argument #2 ($flags) must be of type int, string given\n",
            "htmlspecialchars(): Argument #3 ($encoding) must be of type ?string, StrictStringable given\n",
            "htmlspecialchars(): Argument #4 ($double_encode) must be of type bool, int given\n",
            "htmlentities(): Argument #1 ($string) must be of type string, StrictStringable given\n",
            "get_html_translation_table(): Argument #3 ($encoding) must be of type string, null given\n",
        )
    );
}
