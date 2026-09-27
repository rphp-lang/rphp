mod common;

use common::run_php;

#[test]
fn mbstring_phpunit_surface_has_php_85_identity_order_and_signatures() {
    assert_eq!(
        run_php(
            r#"<?php
echo (int) extension_loaded('mbstring'), (int) extension_loaded('MBSTRING'),
    (int) in_array('mbstring', get_loaded_extensions(), true), "\n";
echo implode(',', get_extension_funcs('mbstring')), "\n";
foreach (get_extension_funcs('mbstring') as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), ':',
        $function->getReturnType(), '|';
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), '=', $parameter->getType(),
            $parameter->isDefaultValueAvailable() ? '=' . var_export($parameter->getDefaultValue(), true) : '', ',';
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "111\n",
            "mb_parse_str,mb_strlen,mb_stripos,mb_substr,mb_convert_encoding,mb_strtolower,mb_detect_encoding,mb_check_encoding,mb_ord\n",
            "mb_parse_str:mbstring:2/2:bool|string=string,result=,\n",
            "mb_strlen:mbstring:1/2:int|string=string,encoding=?string=NULL,\n",
            "mb_stripos:mbstring:2/4:int|false|haystack=string,needle=string,offset=int=0,encoding=?string=NULL,\n",
            "mb_substr:mbstring:2/4:string|string=string,start=int,length=?int=NULL,encoding=?string=NULL,\n",
            "mb_convert_encoding:mbstring:2/3:array|string|false|string=array|string,to_encoding=string,from_encoding=array|string|null=NULL,\n",
            "mb_strtolower:mbstring:1/2:string|string=string,encoding=?string=NULL,\n",
            "mb_detect_encoding:mbstring:1/3:string|false|string=string,encodings=array|string|null=NULL,strict=bool=false,\n",
            "mb_check_encoding:mbstring:0/2:bool|value=array|string|null=NULL,encoding=?string=NULL,\n",
            "mb_ord:mbstring:1/2:int|false|string=string,encoding=?string=NULL,\n",
        )
    );
}

#[test]
fn mbstring_parse_str_shares_php_query_projection_and_writes_the_reference() {
    assert_eq!(
        run_php(
            r#"<?php
$result = ['old' => new stdClass];
var_dump(mb_parse_str('a=b&x[]=1&x[]=2&dot.key=z&plus=a+b', $result));
var_dump($result);
mb_parse_str('', $result);
var_dump($result);
"#,
        ),
        concat!(
            "bool(true)\n",
            "array(4) {\n",
            "  [\"a\"]=>\n  string(1) \"b\"\n",
            "  [\"x\"]=>\n  array(2) {\n",
            "    [0]=>\n    string(1) \"1\"\n",
            "    [1]=>\n    string(1) \"2\"\n",
            "  }\n",
            "  [\"dot_key\"]=>\n  string(1) \"z\"\n",
            "  [\"plus\"]=>\n  string(3) \"a b\"\n",
            "}\n",
            "array(0) {\n}\n",
        )
    );
}

#[test]
fn mbstring_utf8_length_slice_and_codepoint_contracts_are_character_based() {
    assert_eq!(
        run_php(
            r#"<?php
$values = ['', 'ascii', 'žluťoučký', "😀x", "A\xffB"];
foreach ($values as $value) {
    echo bin2hex($value), ':', mb_strlen($value, 'UTF-8'), ':';
    foreach ([[0, null], [1, 2], [-2, null], [0, -1], [-9, 2], [99, null]] as [$start, $length]) {
        echo bin2hex(mb_substr($value, $start, $length, 'UTF-8')), '|';
    }
    echo "\n";
}
foreach ([['A','UTF-8'], ['😀','UTF-8'], ['ž','UTF-8'], ["\x80",'Windows-1252'], ["\xff",'UTF-8']] as [$value,$encoding]) {
    var_dump(mb_ord($value, $encoding));
}
try { mb_ord('', 'UTF-8'); } catch (ValueError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            ":0:||||||\n",
            "6173636969:5:6173636969|7363|6969|61736369|6173||\n",
            "c5be6c75c5a56f75c48d6bc3bd:9:c5be6c75c5a56f75c48d6bc3bd|6c75|6bc3bd|c5be6c75c5a56f75c48d6b|c5be6c||\n",
            "f09f988078:2:f09f988078|78|f09f988078|f09f9880|f09f988078||\n",
            "41ff42:3:413f42|3f42|3f42|413f|413f||\n",
            "int(65)\nint(128512)\nint(382)\nint(8364)\nbool(false)\n",
            "mb_ord(): Argument #1 ($string) must not be empty\n",
        )
    );
}

#[test]
fn mbstring_unicode_lowercase_and_case_insensitive_search_match_phpunit_needs() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['ÄÖÜ STRASSE İΣ', 'Straße', 'ŽLUŤOUČKÝ', 'Σ', 'aΣ', 'aΣb', 'aΣ b', "ab'Σ"] as $value) {
    echo bin2hex(mb_strtolower($value, 'UTF-8')), "\n";
}
foreach ([
    ['Žluťoučký kůň','ŤOU',0], ['Straße','SSE',0], ['😀AbC','abc',0],
    ['abcabc','ABC',-3], ['abc','',0],
] as [$haystack,$needle,$offset]) {
    var_dump(mb_stripos($haystack, $needle, $offset, 'UTF-8'));
}
foreach ([9, -9] as $offset) {
    try { mb_stripos('abc', 'a', $offset, 'UTF-8'); }
    catch (ValueError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "c3a4c3b6c3bc20737472617373652069cc87cf82\n",
            "73747261c39f65\n",
            "c5be6c75c5a56f75c48d6bc3bd\n",
            "cf83\n",
            "61cf82\n",
            "61cf8362\n",
            "61cf822062\n",
            "616227cf82\n",
            "int(3)\nbool(false)\nint(1)\nint(3)\nint(0)\n",
            "mb_stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)\n",
            "mb_stripos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)\n",
        )
    );
}

#[test]
fn mbstring_detection_validation_and_conversion_cover_phpunit_byte_inputs() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    ['ascii', null, true], ['žluť', null, true], ["A\x80B", null, true],
    ["A\x80B", ['Windows-1252','ISO-8859-1'], true],
    ["\x81", ['Windows-1252','ISO-8859-1'], true],
] as [$value,$encodings,$strict]) {
    var_dump(mb_detect_encoding($value, $encodings, $strict));
}
foreach ([
    ["\x80A",'UTF-8','Windows-1252'], ["\x80A",'UTF-8','ISO-8859-1'],
    ['é','Windows-1252','UTF-8'], ["\xff",'UTF-8','UTF-8'],
] as [$value,$to,$from]) {
    echo bin2hex(mb_convert_encoding($value, $to, $from)), "\n";
}
$array = ['x' => "\x80", 2 => 'A', 'nested' => ["\xe9"]];
$converted = mb_convert_encoding($array, 'UTF-8', 'Windows-1252');
echo bin2hex($converted['x']), ':', $converted[2], ':', bin2hex($converted['nested'][0]), "\n";
var_dump(
    mb_check_encoding('žluť', 'UTF-8'),
    mb_check_encoding("A\xffB", 'UTF-8'),
    mb_check_encoding(['ok', 'nested' => ["\xff"]], 'UTF-8'),
    mb_check_encoding()
);
"#,
        ),
        concat!(
            "string(5) \"ASCII\"\n",
            "string(5) \"UTF-8\"\n",
            "bool(false)\n",
            "string(12) \"Windows-1252\"\n",
            "string(12) \"Windows-1252\"\n",
            "e282ac41\n",
            "c28041\n",
            "e9\n",
            "3f\n",
            "e282ac:A:c3a9\n",
            "bool(true)\nbool(false)\nbool(false)\nbool(true)\n",
        )
    );
}

#[test]
fn mbstring_invalid_encoding_diagnostics_are_argument_specific() {
    assert_eq!(
        run_php(
            r#"<?php
$calls = [
    fn() => mb_check_encoding('x', 'BOGUS'),
    fn() => mb_detect_encoding('x', ['BOGUS'], true),
    fn() => mb_detect_encoding('x', [], true),
    fn() => mb_convert_encoding('x', 'BOGUS', 'UTF-8'),
    fn() => mb_convert_encoding('x', 'UTF-8', 'BOGUS'),
];
foreach ($calls as $call) {
    try { $call(); } catch (ValueError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "mb_check_encoding(): Argument #2 ($encoding) must be a valid encoding, \"BOGUS\" given\n",
            "mb_detect_encoding(): Argument #2 ($encodings) contains invalid encoding \"BOGUS\"\n",
            "mb_detect_encoding(): Argument #2 ($encodings) must specify at least one encoding\n",
            "mb_convert_encoding(): Argument #2 ($to_encoding) must be a valid encoding, \"BOGUS\" given\n",
            "mb_convert_encoding(): Argument #3 ($from_encoding) contains invalid encoding \"BOGUS\"\n",
        )
    );
}
