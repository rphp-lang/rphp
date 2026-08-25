mod common;

use common::run_php;

#[test]
fn checksum_hashes_php_bytes_and_preserves_raw_result_identity() {
    assert_eq!(
        run_php(
            r#"<?php
function digest(string $label, string $value): void {
    echo $label, '=', strlen($value), ':', bin2hex($value), "\n";
}
$inputs = [
    'empty' => '',
    'text' => 'checksum lane',
    'binary' => "a\0\xffz",
    'edge' => str_repeat('q', 65),
];
foreach ($inputs AS $label => $input) {
    echo 'crc-', $label, '=', crc32($input), ':', dechex(crc32($input)), "\n";
    digest('sha-'.$label, sha1($input, true));
}
$source = "\x80middle\0tail";
$alias =& $source;
$raw = sha1($source, true);
$source[0] = 'Q';
echo 'cow=', bin2hex($source), ':', bin2hex($alias), ':', strlen($raw), '/', bin2hex($raw), "\n";
"#,
        ),
        r#"crc-empty=0:0
sha-empty=20:da39a3ee5e6b4b0d3255bfef95601890afd80709
crc-text=1074860217:401110b9
sha-text=20:184b813dadd5b407c44692826da5be14cefde813
crc-binary=2167024170:812a2a2a
sha-binary=20:6b419a441881c5640e2654f6f0e553c37da893e0
crc-edge=425929956:19632ce4
sha-edge=20:b0931a65ae5cf3e027199de5f7c56eb0f073c552
cow=516d6964646c65007461696c:516d6964646c65007461696c:20/20f878c7e256424b654d3d8a76e95e10eff5f13a
"#,
    );
}

#[test]
fn checksum_hash_owns_weak_strict_and_diagnostic_boundaries() {
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
        echo is_string($value) ? strlen($value).':'.bin2hex($value) : (string) $value;
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
class DigestText {
    public function __toString(): string { echo "convert\n"; return 'object-text'; }
}
attempt('crc-int', static fn () => crc32(120));
attempt('crc-null', static fn () => crc32(null));
attempt('crc-object', static fn () => crc32(new DigestText()));
attempt('crc-array', static fn () => crc32([]));
attempt('sha-null', static fn () => sha1(null));
attempt('sha-convert-order', static fn () => sha1(new DigestText(), new stdClass()));
attempt('sha-binary-null', static fn () => sha1('x', null));
attempt('sha-binary-text', static fn () => sha1('x', 'yes'));
attempt('dechex-float', static fn () => dechex(15.9));
attempt('dechex-null', static fn () => dechex(null));
attempt('dechex-array', static fn () => dechex([]));
restore_error_handler();
"#,
        ),
        r#"crc-int=289485416
crc-null=diag=8192:crc32(): Passing null to parameter #1 ($string) of type string is deprecated
0
crc-object=convert
629942395
crc-array=TypeError:crc32(): Argument #1 ($string) must be of type string, array given
sha-null=diag=8192:sha1(): Passing null to parameter #1 ($string) of type string is deprecated
40:64613339613365653565366234623064333235356266656639353630313839306166643830373039
sha-convert-order=convert
TypeError:sha1(): Argument #2 ($binary) must be of type bool, stdClass given
sha-binary-null=diag=8192:sha1(): Passing null to parameter #2 ($binary) of type bool is deprecated
40:31316636616438656335326132393834616261616664376333623531363530333738356332303732
sha-binary-text=20:11f6ad8ec52a2984abaafd7c3b516503785c2072
dechex-float=diag=8192:Implicit conversion from float 15.9 to int loses precision
1:66
dechex-null=diag=8192:dechex(): Passing null to parameter #1 ($num) of type int is deprecated
1:30
dechex-array=TypeError:dechex(): Argument #1 ($num) must be of type int, array given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { $value = $call(); echo is_string($value) ? bin2hex($value) : (string) $value; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
attempt('crc-valid', static fn () => crc32('x'));
attempt('crc-int', static fn () => crc32(120));
attempt('sha-valid', static fn () => sha1('x', true));
attempt('sha-int', static fn () => sha1(120));
attempt('sha-bool', static fn () => sha1('x', 1));
attempt('dechex-int', static fn () => dechex(-1));
attempt('dechex-string', static fn () => dechex('15'));
"#,
        ),
        r#"crc-valid=2363233923
crc-int=TypeError:crc32(): Argument #1 ($string) must be of type string, int given
sha-valid=11f6ad8ec52a2984abaafd7c3b516503785c2072
sha-int=TypeError:sha1(): Argument #1 ($string) must be of type string, int given
sha-bool=TypeError:sha1(): Argument #2 ($binary) must be of type bool, int given
dechex-int=66666666666666666666666666666666
dechex-string=TypeError:dechex(): Argument #1 ($num) must be of type int, string given
"#,
    );
}

#[test]
fn checksum_hash_file_calls_and_reflection_share_one_contract() {
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
        if (is_string($value)) echo strlen($value), ':', bin2hex($value);
        elseif ($value === false) echo 'false';
        else var_dump($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
$path = 'rphp-checksum-contract-data.bin';
file_put_contents($path, "file\0\xffdata\n");
attempt('md5-hex', static fn () => md5_file($GLOBALS['path']));
attempt('md5-raw', static fn () => md5_file($GLOBALS['path'], true));
attempt('sha-hex', static fn () => sha1_file($GLOBALS['path']));
attempt('sha-raw', static fn () => sha1_file($GLOBALS['path'], true));
attempt('md5-empty', static fn () => md5_file(''));
attempt('sha-missing', static fn () => sha1_file('rphp-checksum-contract-missing.bin'));
attempt('sha-null', static fn () => sha1_file(null));
attempt('validate-before-open', static fn () => md5_file('rphp-checksum-contract-missing.bin', new stdClass()));

$name = 'sha1';
$callback = sha1(...);
attempt('named', static fn () => sha1(string: 'abc', binary: true));
attempt('dynamic', static fn () => ($GLOBALS['name'])('abc', true));
attempt('callback', static fn () => ($GLOBALS['callback'])('abc', true));
attempt('call-user', static fn () => call_user_func('crc32', 'abc'));
attempt('call-array', static fn () => call_user_func_array('sha1', ['binary' => true, 'string' => 'abc']));
attempt('too-many', static fn () => sha1('abc', false, 1));
attempt('unknown', static fn () => crc32(string: 'abc', extra: 1));

foreach (['crc32', 'sha1', 'md5_file', 'sha1_file', 'dechex'] AS $function) {
    $reflection = new ReflectionFunction($function);
    echo 'reflection=', $function, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
    foreach ($reflection->getParameters() as $parameter) {
        echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
            $parameter->isOptional() ? 'optional' : 'required', ':',
            $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
    }
}
unlink($path);
restore_error_handler();
"#,
        ),
        r#"md5-hex=32:3930366436623264363564386437343634353536663964626366376235386332
md5-raw=16:906d6b2d65d8d7464556f9dbcf7b58c2
sha-hex=40:35313031363232356237373666373132306638363633366365646266363231386638333031356133
sha-raw=20:51016225b776f7120f86636cedbf6218f83015a3
md5-empty=ValueError:Path must not be empty
sha-missing=diag=2:sha1_file(rphp-checksum-contract-missing.bin): Failed to open stream: No such file or directory
false
sha-null=diag=8192:sha1_file(): Passing null to parameter #1 ($filename) of type string is deprecated
ValueError:Path must not be empty
validate-before-open=TypeError:md5_file(): Argument #2 ($binary) must be of type bool, stdClass given
named=20:a9993e364706816aba3e25717850c26c9cd0d89d
dynamic=20:a9993e364706816aba3e25717850c26c9cd0d89d
callback=20:a9993e364706816aba3e25717850c26c9cd0d89d
call-user=int(891568578)

call-array=20:a9993e364706816aba3e25717850c26c9cd0d89d
too-many=ArgumentCountError:sha1() expects at most 2 arguments, 3 given
unknown=Error:Unknown named parameter $extra
reflection=crc32:1/1:int
param=string:string:required:nonnull
reflection=sha1:1/2:string
param=string:string:required:nonnull
param=binary:bool:optional:nonnull
reflection=md5_file:1/2:string|false
param=filename:string:required:nonnull
param=binary:bool:optional:nonnull
reflection=sha1_file:1/2:string|false
param=filename:string:required:nonnull
param=binary:bool:optional:nonnull
reflection=dechex:1/1:string
param=num:int:required:nonnull
"#,
    );
}
