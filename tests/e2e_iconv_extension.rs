#![cfg(target_os = "linux")]

mod common;

use common::run_php;

#[test]
fn iconv_exports_php_85_signatures_constants_and_extension_identity() {
    let output = run_php(
        r#"<?php
$names = [
    'iconv_strlen','iconv_substr','iconv_strpos','iconv_strrpos',
    'iconv_mime_encode','iconv_mime_decode','iconv_mime_decode_headers',
    'iconv','iconv_set_encoding','iconv_get_encoding',
];
foreach ($names as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', (string) $function->getReturnType(), '|';
}
echo "\n", ICONV_IMPL, '|', ICONV_VERSION, '|', ICONV_MIME_DECODE_STRICT,
    ICONV_MIME_DECODE_CONTINUE_ON_ERROR, '|',
    (int) extension_loaded('iconv'), (int) extension_loaded('ICONV'), '|',
    implode(',', get_loaded_extensions()), "\n";
"#,
    );
    let (signatures, identity) = output
        .trim_end()
        .split_once('\n')
        .expect("iconv identity follows the signature inventory");
    assert_eq!(
        signatures,
        concat!(
            "iconv_strlen:iconv:1/2:int|false|iconv_substr:iconv:2/4:string|false|",
            "iconv_strpos:iconv:2/4:int|false|iconv_strrpos:iconv:2/3:int|false|",
            "iconv_mime_encode:iconv:2/3:string|false|iconv_mime_decode:iconv:1/3:string|false|",
            "iconv_mime_decode_headers:iconv:1/3:array|false|iconv:iconv:3/3:string|false|",
            "iconv_set_encoding:iconv:2/2:bool|iconv_get_encoding:iconv:0/1:array|string|false|",
        )
    );
    let identity: Vec<_> = identity.split('|').collect();
    assert_eq!(identity.len(), 5);
    if cfg!(target_env = "gnu") {
        assert_eq!(identity[0], "glibc");
        assert!(
            !identity[1].is_empty()
                && identity[1]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
                && identity[1].bytes().any(|byte| byte.is_ascii_digit())
        );
    } else {
        assert_eq!(&identity[..2], ["unknown", "unknown"]);
    }
    assert_eq!(&identity[2..], ["12", "11", "calendar,gettext,iconv"]);
}

#[test]
fn iconv_parameter_names_types_and_defaults_are_exact() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['iconv_substr','iconv_strpos','iconv_mime_encode','iconv_get_encoding'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, '(';
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), ':', (string) $parameter->getType();
        if ($parameter->isDefaultValueAvailable()) {
            echo '=', var_export($parameter->getDefaultValue(), true);
        }
        echo ',';
    }
    echo ")\n";
}
"#,
        ),
        concat!(
            "iconv_substr(string:string,offset:int,length:?int=NULL,encoding:?string=NULL,)\n",
            "iconv_strpos(haystack:string,needle:string,offset:int=0,encoding:?string=NULL,)\n",
            "iconv_mime_encode(field_name:string,field_value:string,options:array=array (\n),)\n",
            "iconv_get_encoding(type:string='all',)\n",
        )
    );
}

#[test]
fn iconv_mime_encode_validates_its_optional_array_before_dispatch() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    iconv_mime_encode('Subject', 'x', null);
} catch (Throwable $error) {
    echo get_class($error), ': ', $error->getMessage(), "\n";
}
var_dump(iconv_mime_encode('Subject', 'x'));
"#,
        ),
        concat!(
            "TypeError: iconv_mime_encode(): Argument #3 ($options) must be of type array, null given\n",
            "string(25) \"Subject: =?UTF-8?B?eA==?=\"\n",
        )
    );
}

#[test]
fn iconv_native_conversion_preserves_binary_transliteration_and_ignore_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
setlocale(LC_CTYPE, 'C.UTF-8');
echo bin2hex(iconv('ISO-8859-1', 'UTF-8', "Pr\xFCfung")), '|',
    iconv('UTF-8', 'ASCII//TRANSLIT', 'Žluťoučký kůň'), '|',
    bin2hex(iconv('UTF-8', 'UTF-16LE', "A\0B")), "\n";
echo urlencode(iconv('UTF-8', 'UTF-8//IGNORE', "aa\xC3\xC3\xC3\xB8aa")), "\n";
set_error_handler(static function(int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
var_dump(iconv('missing-rphp', 'UTF-8', 'x'));
var_dump(iconv('UTF-8', 'UTF-8', "\xC3"));
"#,
        ),
        concat!(
            "5072c3bc66756e67|Zlutoucky kun|410000004200\n",
            "aa%C3%B8aa\n",
            "2:iconv(): Wrong encoding, conversion from \"missing-rphp\" to \"UTF-8\" is not allowed\n",
            "bool(false)\n",
            "8:iconv(): Detected an incomplete multibyte character in input string\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn iconv_character_operations_use_codepoint_offsets_and_php_bounds() {
    assert_eq!(
        run_php(
            r#"<?php
$value = '日本語テストです。01234５６７８９。';
echo iconv_strlen($value, 'UTF-8'), '|', iconv_substr($value, 2, 7, 'UTF-8'), '|',
    iconv_substr($value, -4, -1, 'UTF-8'), "\n";
echo iconv_strpos($value, 'テスト', 0, 'UTF-8'), '|',
    iconv_strpos($value, 'テスト', -19, 'UTF-8'), '|',
    iconv_strrpos('abc日本abc日本', '日本', 'UTF-8'), "\n";
foreach ([[3, null],[-3, null],[4, null],[-4, 1],[0, -4],[2, -2]] as [$offset, $length]) {
    echo '[', iconv_substr('foo', $offset, $length, 'UTF-8'), ']';
}
echo "\n";
try { iconv_strpos('abc', 'a', 4, 'UTF-8'); }
catch (ValueError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "20|語テストです。|７８９\n",
            "3|3|8\n",
            "[][foo][][f][][]\n",
            "iconv_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)\n",
        )
    );
}

#[test]
fn iconv_encoding_state_is_request_local_and_queryable() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL & ~E_DEPRECATED);
echo implode('|', iconv_get_encoding('all')), "\n";
foreach (['input_encoding','output_encoding','internal_encoding'] as $type) {
    echo (int) iconv_set_encoding($type, 'ISO-8859-1'), '|', iconv_get_encoding($type), "\n";
}
echo implode('|', iconv_get_encoding()), '|';
var_dump(iconv_get_encoding('missing'));
"#,
        ),
        concat!(
            "UTF-8|UTF-8|UTF-8\n",
            "1|ISO-8859-1\n1|ISO-8859-1\n1|ISO-8859-1\n",
            "ISO-8859-1|ISO-8859-1|ISO-8859-1|bool(false)\n",
        )
    );
}

#[test]
fn iconv_ini_fallback_and_deprecated_setter_share_request_local_state() {
    assert_eq!(
        run_php(
            r#"<?php
echo ini_get('default_charset'), '|', ini_get('iconv.internal_encoding'), "\n";
var_dump(ini_get('iconv.internal_charset'), ini_get('iconv.http_input'), ini_get('iconv.http_output'));
var_dump(
    ini_set('iconv.internal_charset', 'ISO-8859-1'),
    ini_set('iconv.http_input', 'ISO-8859-1'),
    ini_set('iconv.http_output', 'ISO-8859-1'),
);
var_dump(ini_set('default_charset', 'ISO-8859-1'));
echo iconv_get_encoding('internal_encoding'), "\n";
set_error_handler(static function(int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
var_dump(iconv_set_encoding('internal_encoding', 'UTF-8'));
echo iconv_get_encoding('internal_encoding'), '|', ini_get('iconv.internal_encoding'), "\n";
"#,
        ),
        concat!(
            "UTF-8|\n",
            "bool(false)\nbool(false)\nbool(false)\n",
            "bool(false)\nbool(false)\nbool(false)\n",
            "string(5) \"UTF-8\"\n",
            "ISO-8859-1\n",
            "8192:iconv_set_encoding(): Use of iconv.internal_encoding is deprecated\n",
            "bool(true)\n",
            "UTF-8|UTF-8\n",
        )
    );
}

#[test]
fn iconv_mime_q_b_and_header_projection_cover_shared_rfc_2047_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
echo iconv_mime_decode('Subject: =?ISO-8859-1?Q?Pr=FCfung?=', 0, 'UTF-8'), "\n";
echo iconv_mime_decode('=?UTF-8?B?5pel5pys6Kqe?= =?UTF-8?Q?=E3=83=86=E3=82=B9=E3=83=88?=', 0, 'UTF-8'), "\n";
echo iconv_mime_encode('Subject', 'Prüfung test', [
    'input-charset' => 'UTF-8', 'output-charset' => 'UTF-8', 'scheme' => 'Q',
]), "\n";
$euc = iconv('UTF-8', 'EUC-JP', 'サンプル');
echo iconv_mime_encode('From', $euc, [
    'input-charset' => 'EUC-JP', 'output-charset' => 'ISO-2022-JP',
    'scheme' => 'B', 'line-length' => 39, 'line-break-chars' => "\n",
]), "\n";
$headers = "Subject: =?ISO-8859-1?Q?Pr=FCfung?=\r\n"
    . "Received: first\r\n\tcontinued\r\nReceived: second\r\n";
var_export(iconv_mime_decode_headers($headers, 0, 'UTF-8'));
echo "\n";
"#,
        ),
        concat!(
            "Subject: Prüfung\n",
            "日本語テスト\n",
            "Subject: =?UTF-8?Q?Pr=C3=BCfung=20test?=\n",
            "From: =?ISO-2022-JP?B?GyRCJTUbKEI=?=\n",
            " =?ISO-2022-JP?B?GyRCJXMlVyVrGyhC?=\n",
            "array (\n",
            "  'Subject' => 'Prüfung',\n",
            "  'Received' => \n",
            "  array (\n",
            "    0 => 'first continued',\n",
            "    1 => 'second',\n",
            "  ),\n",
            ")\n",
        )
    );
}

#[test]
fn iconv_mime_ignore_modifier_does_not_hide_plain_invalid_bytes() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function(int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
var_dump(iconv_mime_decode("\xFF", 0, 'UTF-8//IGNORE'));
"#,
        ),
        concat!(
            "8:iconv_mime_decode(): Detected an illegal character in input string\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn iconv_rejects_overlong_encodings_before_native_calls() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function(int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
$encoding = str_repeat('A', 64);
var_dump(iconv($encoding, 'UTF-8', 'x'));
var_dump(iconv_strlen('x', $encoding));
var_dump(iconv_set_encoding('internal_encoding', $encoding));
"#,
        ),
        concat!(
            "2:iconv(): Encoding parameter exceeds the maximum allowed length of 64 characters\n",
            "bool(false)\n",
            "2:iconv_strlen(): Encoding parameter exceeds the maximum allowed length of 64 characters\n",
            "bool(false)\n",
            "2:iconv_set_encoding(): Encoding parameter exceeds the maximum allowed length of 64 characters\n",
            "bool(false)\n",
        )
    );
}
