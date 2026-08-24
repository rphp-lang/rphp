//! Original PHP 8.5 differential boundaries for pack()/unpack().

mod common;
use common::run_php;

#[test]
fn integer_width_endian_sign_and_truncation_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    ['c', -128], ['C', 255], ['C', 256],
    ['s', -32768], ['S', 65535], ['n', 0x1234], ['v', 0x1234],
    ['i', -2147483648], ['I', 4294967295],
    ['l', -2147483648], ['L', 4294967295],
    ['N', 0x01020304], ['V', 0x01020304],
    ['q', -9223372036854775807 - 1], ['Q', -1], ['J', 0x0102030405060708],
    ['P', 0x0102030405060708],
];
foreach ($cases as [$format, $input]) {
    $bytes = pack($format, $input);
    echo $format, '=', bin2hex($bytes), ':', unpack($format, $bytes)[1], "\n";
}
"#
        ),
        concat!(
            "c=80:-128\n",
            "C=ff:255\n",
            "C=00:0\n",
            "s=0080:-32768\n",
            "S=ffff:65535\n",
            "n=1234:4660\n",
            "v=3412:4660\n",
            "i=00000080:-2147483648\n",
            "I=ffffffff:4294967295\n",
            "l=00000080:-2147483648\n",
            "L=ffffffff:4294967295\n",
            "N=01020304:16909060\n",
            "V=04030201:16909060\n",
            "q=0000000000000080:-9223372036854775808\n",
            "Q=ffffffffffffffff:-1\n",
            "J=0102030405060708:72623859790382856\n",
            "P=0807060504030201:72623859790382856\n",
        )
    );
}

#[test]
fn float_endian_infinity_nan_and_negative_zero_contract() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['f', 'g', 'G', 'd', 'e', 'E'] as $format) {
    $negativeZero = pack($format, -0.0);
    $infinity = pack($format, INF);
    $nan = unpack($format, pack($format, NAN))[1];
    echo $format, '=', bin2hex($negativeZero), ',', bin2hex($infinity), ',',
        is_nan($nan) ? 'nan' : 'not-nan', "\n";
}
"#
        ),
        concat!(
            "f=00000080,0000807f,nan\n",
            "g=00000080,0000807f,nan\n",
            "G=80000000,7f800000,nan\n",
            "d=0000000000000080,000000000000f07f,nan\n",
            "e=0000000000000080,000000000000f07f,nan\n",
            "E=8000000000000000,7ff0000000000000,nan\n",
        )
    );
}

#[test]
fn text_hex_padding_nibble_and_cursor_contract() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    ['a5', "a\0b"], ['A5', "a\0b"], ['Z5', "a\0b"],
    ['h5', '12345'], ['H5', '12345'],
] as [$format, $input]) {
    echo $format, '=', bin2hex(pack($format, $input)), "\n";
}
echo 'cursor=', bin2hex(pack('CCX@4C', 1, 2, 3)), "\n";
echo 'utf8=', bin2hex(pack('a*', 'ž')), "\n";
echo 'raw=', bin2hex(unpack('a5raw', "a\0b\0\0")['raw']), "\n";
echo 'blank=', bin2hex(unpack('A5blank', "a\0b \t")['blank']), "\n";
echo 'term=', bin2hex(unpack('Z5term', "a\0bcd")['term']), "\n";
echo 'low=', unpack('h5low', hex2bin('214305'))['low'], "\n";
echo 'high=', unpack('H5high', hex2bin('123450'))['high'], "\n";
"#
        ),
        concat!(
            "a5=6100620000\n",
            "A5=6100622020\n",
            "Z5=6100620000\n",
            "h5=214305\n",
            "H5=123450\n",
            "cursor=0100000003\n",
            "utf8=c5be\n",
            "raw=6100620000\n",
            "blank=610062\n",
            "term=61\n",
            "low=12345\n",
            "high=12345\n",
        )
    );
}

#[test]
fn repeat_star_names_suffixes_and_unnamed_overwrite_contract() {
    assert_eq!(
        run_php(
            r#"<?php
echo bin2hex(pack('C*', 65, 66, 67)), "\n";
foreach (unpack('C2named/C*tail', 'ABCDE') as $key => $value) {
    echo $key, '=', $value, ';';
}
echo "\n";
foreach (unpack('C2/C', 'ABC') as $key => $value) {
    echo $key, '=', $value, ';';
}
echo "\n";
foreach (unpack('a*text', "a\0b") as $key => $value) {
    echo $key, '=', bin2hex($value), ';';
}
"#
        ),
        concat!(
            "414243\n",
            "named1=65;named2=66;tail1=67;tail2=68;tail3=69;\n",
            "1=67;2=66;\n",
            "text=610062;",
        )
    );
}

#[test]
fn empty_short_excess_and_format_diagnostics_contract() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo 'warning:', $message, "\n";
    return true;
});
echo 'empty=', bin2hex(pack('', 1)), "\n";
echo 'excess=', bin2hex(pack('C', 1, 2, 3)), "\n";
try { pack('C2', 1); } catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
try { pack('?', 1); } catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
var_dump(unpack('', ''));
var_dump(unpack('C*', ''));
var_dump(unpack('C2', "\x01"));
var_dump(unpack('h2147483648', ''));
try { unpack('?', 'x'); } catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#
        ),
        concat!(
            "empty=warning:pack(): 1 arguments unused\n",
            "\n",
            "excess=warning:pack(): 2 arguments unused\n",
            "01\n",
            "ValueError:Type C: too few arguments\n",
            "ValueError:Type ?: unknown format code\n",
            "array(0) {\n}\n",
            "array(0) {\n}\n",
            "warning:unpack(): Type C: not enough input values, need 1 values but only 0 were provided\n",
            "bool(false)\n",
            "warning:unpack(): Type h: integer overflow\n",
            "bool(false)\n",
            "ValueError:Invalid format type ?\n",
        )
    );
}

#[test]
fn offsets_accept_boundaries_and_reject_outside_data() {
    assert_eq!(
        run_php(
            r#"<?php
$data = pack('C3', 10, 20, 30);
echo unpack('C', $data, 0)[1], ',', unpack('C', $data, 2)[1], "\n";
var_dump(unpack('', $data, strlen($data)));
foreach ([-1, 4] as $offset) {
    try { unpack('C', $data, $offset); } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
echo unpack('C/@0/C', 'ABCDE', 2)[1], "\n";
echo unpack('C/@4/C', 'ABCDEFG', 2)[1], "\n";
echo unpack('C/X9/C', 'ABCDE', 2)[1], "\n";
"#
        ),
        concat!(
            "10,30\n",
            "array(0) {\n}\n",
            "ValueError:unpack(): Argument #3 ($offset) must be contained in argument #2 ($data)\n",
            "ValueError:unpack(): Argument #3 ($offset) must be contained in argument #2 ($data)\n",
            "68\n",
            "71\n",
            "67\n",
        )
    );
}

#[test]
fn weak_strict_named_variadic_and_reflection_contract() {
    assert_eq!(
        run_php(
            r#"<?php
echo bin2hex(pack('C3', ...['65', true, null])), "\n";
echo strlen(pack('C2', 0, 255)), "\n";
echo unpack('Cvalue', 65, '0')['value'], "\n";
try { pack(format: 'C', values: 65); } catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
foreach (['pack', 'unpack'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), ':',
        $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), '=', $parameter->getType(), ':',
            (int) $parameter->isVariadic(), ';';
    }
    echo "\n";
}
"#
        ),
        concat!(
            "410100\n",
            "2\n",
            "54\n",
            "ArgumentCountError:pack() does not accept unknown named parameters\n",
            "pack:1:2:string\n",
            "format=string:0;values=mixed:1;\n",
            "unpack:2:3:array|false\n",
            "format=string:0;string=string:0;offset=int:0;\n",
        )
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
foreach ([
    fn() => pack(123, 1),
    fn() => unpack(123, 'x'),
    fn() => unpack('C', 123),
    fn() => unpack('C', 'x', '0'),
] as $call) {
    try { $call(); } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#
        ),
        concat!(
            "TypeError:pack(): Argument #1 ($format) must be of type string, int given\n",
            "TypeError:unpack(): Argument #1 ($format) must be of type string, int given\n",
            "TypeError:unpack(): Argument #2 ($string) must be of type string, int given\n",
            "TypeError:unpack(): Argument #3 ($offset) must be of type int, string given\n",
        )
    );
}
