mod common;

use common::run_php;

#[test]
fn quoted_printable_matches_php_bytes_wrapping_and_roundtrips() {
    assert_eq!(
        run_php(
            r#"<?php
function bytes(string $label, string $value): void {
    echo $label, '=', strlen($value), ':', bin2hex($value), "\n";
}
$encodeCases = [
    'empty' => '',
    'printable' => 'Az09!~',
    'equals-tab' => "A=\tZ",
    'crlf' => "A\r\nZ",
    'single-lines' => "A\nB\rC",
    'trailing-space' => "A \r\nZ ",
    'binary' => "\0\x7f\xff",
];
foreach ($encodeCases as $label => $input) {
    bytes('enc/'.$label, quoted_printable_encode($input));
}
$utf = quoted_printable_encode(str_repeat("\xc4\x85", 13));
echo 'utf-boundary=', strlen($utf), ':', strpos($utf, "=\r\n"), ':', sha1($utf), "\n";
$nul = quoted_printable_encode(str_repeat("\0", 26));
echo 'nul-boundary=', strlen($nul), ':', strpos($nul, "=\r\n"), ':', sha1($nul), "\n";
$unicode = str_repeat('řeka', 20);
$unicodeEncoded = quoted_printable_encode($unicode);
echo 'unicode=', strlen($unicode), ':', strlen($unicodeEncoded), ':',
    sha1($unicodeEncoded), ':',
    sha1(quoted_printable_decode($unicodeEncoded)) === sha1($unicode) ? 'same' : 'different', "\n";

$decodeCases = [
    'hex' => '=00=7f=Af=ff',
    'soft' => "a= \t\r\nb=\nc=\rd",
    'malformed' => 'A=G==41=B',
    'trim-tail' => "before= \t",
    'raw-nul' => "A\0ignored",
    'raw-high' => "A\xffZ",
];
foreach ($decodeCases as $label => $input) {
    bytes('dec/'.$label, quoted_printable_decode($input));
}
$roundtrip = str_repeat("A\0\xff\xc4\x85\r\n", 40);
$roundtripEncoded = quoted_printable_encode($roundtrip);
echo 'roundtrip=', strlen($roundtripEncoded), ':', sha1($roundtripEncoded), ':',
    quoted_printable_decode($roundtripEncoded) === $roundtrip ? 'same' : 'different', "\n";

$raw = quoted_printable_decode('=FF=00=41');
$copy = $raw;
$raw[1] = 'Q';
$reference =& $raw;
$reference[0] = 'B';
echo 'cow=', bin2hex($raw), ':', bin2hex($copy), ':', bin2hex($reference), "\n";
"#,
        ),
        r#"enc/empty=0:
enc/printable=6:417a3039217e
enc/equals-tab=8:413d33443d30395a
enc/crlf=4:410d0a5a
enc/single-lines=9:413d3041423d304443
enc/trailing-space=8:413d32300d0a5a20
enc/binary=9:3d30303d37463d4646
utf-boundary=81:72:d5f4e1e164594d3dabe55dedbdb6dd8e103062ca
nul-boundary=81:75:7621e02cfaaae5b7e63620ea74516779c9128457
unicode=100:186:19aafd9f2ecea35ba04c1c51e4a55c1144497a6a:same
dec/hex=4:007fafff
dec/soft=4:61626364
dec/malformed=7:413d473d413d42
dec/trim-tail=6:6265666f7265
dec/raw-nul=1:41
dec/raw-high=3:41ff5a
roundtrip=600:234c998b4ac863a994955a827be562b9d4587c3d:same
cow=425141:ff0041:425141
"#,
    );
}

#[test]
fn quoted_printable_owns_weak_strict_and_diagnostic_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo strlen($value), ':', bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
class PrintableText {
    public function __toString(): string { echo "convert\n"; return '=41'; }
}
foreach (['quoted_printable_encode', 'quoted_printable_decode'] as $name) {
    echo "function=$name\n";
    attempt('int', static fn () => $GLOBALS['name'](120));
    attempt('null', static fn () => $GLOBALS['name'](null));
    attempt('object', static fn () => $GLOBALS['name'](new PrintableText()));
    attempt('array', static fn () => $GLOBALS['name']([]));
}
restore_error_handler();
"#,
        ),
        r#"function=quoted_printable_encode
int=3:313230
null=diag=8192:quoted_printable_encode(): Passing null to parameter #1 ($string) of type string is deprecated
0:
object=convert
5:3d33443431
array=TypeError:quoted_printable_encode(): Argument #1 ($string) must be of type string, array given
function=quoted_printable_decode
int=3:313230
null=diag=8192:quoted_printable_decode(): Passing null to parameter #1 ($string) of type string is deprecated
0:
object=convert
1:41
array=TypeError:quoted_printable_decode(): Argument #1 ($string) must be of type string, array given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo strlen($value), ':', bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
attempt('encode-string', static fn () => quoted_printable_encode('=41'));
attempt('encode-int', static fn () => quoted_printable_encode(120));
attempt('decode-string', static fn () => quoted_printable_decode('=41'));
attempt('decode-bool', static fn () => quoted_printable_decode(true));
"#,
        ),
        r#"encode-string=5:3d33443431
encode-int=TypeError:quoted_printable_encode(): Argument #1 ($string) must be of type string, int given
decode-string=1:41
decode-bool=TypeError:quoted_printable_decode(): Argument #1 ($string) must be of type string, true given
"#,
    );
}

#[test]
fn quoted_printable_call_shapes_and_reflection_share_one_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo strlen($value), ':', bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
foreach (['quoted_printable_encode', 'quoted_printable_decode'] as $name) {
    echo "function=$name\n";
    $callback = $name(...);
    attempt('named', static fn () => $GLOBALS['name'](string: '=41'));
    attempt('dynamic', static fn () => ($GLOBALS['name'])('=41'));
    attempt('callback', static fn () => ($GLOBALS['callback'])('=41'));
    attempt('call-user', static fn () => call_user_func($GLOBALS['name'], '=41'));
    attempt('call-array', static fn () => call_user_func_array($GLOBALS['name'], ['string' => '=41']));
    attempt('missing', static fn () => $GLOBALS['name']());
    attempt('too-many', static fn () => $GLOBALS['name']('A', 1));
    attempt('unknown', static fn () => $GLOBALS['name'](string: 'A', extra: 1));
    $reflection = new ReflectionFunction($name);
    echo 'reflection=', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
    $parameter = $reflection->getParameters()[0];
    echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isOptional() ? 'optional' : 'required', ':',
        $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
}
$source = '=41=42';
$alias =& $source;
$copy = $source;
echo 'refs=', bin2hex(quoted_printable_decode($source)), ':', $source, ':', $alias, ':', $copy, "\n";
"#,
        ),
        r#"function=quoted_printable_encode
named=5:3d33443431
dynamic=5:3d33443431
callback=5:3d33443431
call-user=5:3d33443431
call-array=5:3d33443431
missing=ArgumentCountError:quoted_printable_encode() expects exactly 1 argument, 0 given
too-many=ArgumentCountError:quoted_printable_encode() expects exactly 1 argument, 2 given
unknown=Error:Unknown named parameter $extra
reflection=1/1:string
param=string:string:required:nonnull
function=quoted_printable_decode
named=1:41
dynamic=1:41
callback=1:41
call-user=1:41
call-array=1:41
missing=ArgumentCountError:quoted_printable_decode() expects exactly 1 argument, 0 given
too-many=ArgumentCountError:quoted_printable_decode() expects exactly 1 argument, 2 given
unknown=Error:Unknown named parameter $extra
reflection=1/1:string
param=string:string:required:nonnull
refs=4142:=41=42:=41=42:=41=42
"#,
    );
}
