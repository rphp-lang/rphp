mod common;

use common::run_php;

#[test]
fn base64_decode_matches_php_bytes_padding_and_strictness() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    'empty' => '',
    'one' => 'A',
    'canonical' => 'YQ==',
    'unpadded' => 'YWI',
    'leading-pad' => '=YQ',
    'extra-pad' => 'YQ===',
    'after-pad' => 'YQ==QQ',
    'spaced' => " YQ =\t=\r\n",
    'punctuation' => '!Y@W#J$j%',
    'nul-high' => "Y\0\xffW\x80Jj",
    'only-invalid' => "!\0\xff",
];
foreach ($cases AS $label => $input) {
    foreach ([false, true] AS $strict) {
        $value = base64_decode($input, $strict);
        echo $label, '/', $strict ? 'strict' : 'loose', '=';
        echo $value === false ? 'false' : strlen($value).':'.bin2hex($value);
        echo "\n";
    }
}
$long = base64_decode(str_repeat('YWJj', 33), true);
echo 'long=', strlen($long), ':', sha1($long), "\n";
$raw = base64_decode('/wA=', true);
$copy = $raw;
$raw[1] = 'Q';
$reference =& $raw;
$reference[0] = 'A';
echo 'cow=', strlen($raw), ':', bin2hex($raw), ':', bin2hex($copy), ':', bin2hex($reference), "\n";
"#,
        ),
        r#"empty/loose=0:
empty/strict=0:
one/loose=0:
one/strict=false
canonical/loose=1:61
canonical/strict=1:61
unpadded/loose=2:6162
unpadded/strict=2:6162
leading-pad/loose=1:61
leading-pad/strict=false
extra-pad/loose=1:61
extra-pad/strict=false
after-pad/loose=3:610410
after-pad/strict=false
spaced/loose=1:61
spaced/strict=1:61
punctuation/loose=3:616263
punctuation/strict=false
nul-high/loose=3:616263
nul-high/strict=false
only-invalid/loose=0:
only-invalid/strict=false
long=99:2f0846d7ac1b26a89cf8b2d32a73409ae39d0d80
cow=2:4151:ff00:4151
"#,
    );
}

#[test]
fn base64_decode_owns_weak_strict_and_diagnostic_boundaries() {
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
        echo $value === false ? 'false' : strlen($value).':'.bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
class DecodeText {
    public function __toString(): string { echo "convert\n"; return 'YQ=='; }
}
attempt('int', static fn () => base64_decode(120));
attempt('null-data', static fn () => base64_decode(null));
attempt('object', static fn () => base64_decode(new DecodeText()));
attempt('array', static fn () => base64_decode([]));
attempt('null-strict', static fn () => base64_decode('YQ==', null));
attempt('text-strict', static fn () => base64_decode('YQ==', 'yes'));
attempt('order', static fn () => base64_decode(new DecodeText(), new stdClass()));
restore_error_handler();
"#,
        ),
        r#"int=2:d76d
null-data=diag=8192:base64_decode(): Passing null to parameter #1 ($string) of type string is deprecated
0:
object=convert
1:61
array=TypeError:base64_decode(): Argument #1 ($string) must be of type string, array given
null-strict=diag=8192:base64_decode(): Passing null to parameter #2 ($strict) of type bool is deprecated
1:61
text-strict=1:61
order=convert
TypeError:base64_decode(): Argument #2 ($strict) must be of type bool, stdClass given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo $value === false ? 'false' : strlen($value).':'.bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
attempt('valid', static fn () => base64_decode('YQ==', true));
attempt('int-data', static fn () => base64_decode(120));
attempt('int-strict', static fn () => base64_decode('YQ==', 1));
attempt('string-strict', static fn () => base64_decode('YQ==', '1'));
"#,
        ),
        r#"valid=1:61
int-data=TypeError:base64_decode(): Argument #1 ($string) must be of type string, int given
int-strict=TypeError:base64_decode(): Argument #2 ($strict) must be of type bool, int given
string-strict=TypeError:base64_decode(): Argument #2 ($strict) must be of type bool, string given
"#,
    );
}

#[test]
fn base64_decode_call_shapes_and_reflection_share_the_handler_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo $value === false ? 'false' : strlen($value).':'.bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
$name = 'base64_decode';
$callback = base64_decode(...);
attempt('named', static fn () => base64_decode(string: 'YQ==', strict: true));
attempt('dynamic', static fn () => ($GLOBALS['name'])('YQ==', true));
attempt('callback', static fn () => ($GLOBALS['callback'])('YQ==', true));
attempt('call-user', static fn () => call_user_func('base64_decode', 'YQ==', true));
attempt('call-array', static fn () => call_user_func_array('base64_decode', ['strict' => true, 'string' => 'YQ==']));
attempt('too-many', static fn () => base64_decode('YQ==', false, 1));
attempt('unknown', static fn () => base64_decode(string: 'YQ==', extra: true));
$reflection = new ReflectionFunction('base64_decode');
echo 'reflection=', $reflection->getNumberOfRequiredParameters(), '/',
    $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
foreach ($reflection->getParameters() as $parameter) {
    echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isOptional() ? 'optional' : 'required', ':',
        $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
}
"#,
        ),
        r#"named=1:61
dynamic=1:61
callback=1:61
call-user=1:61
call-array=1:61
too-many=ArgumentCountError:base64_decode() expects at most 2 arguments, 3 given
unknown=Error:Unknown named parameter $extra
reflection=1/2:string|false
param=string:string:required:nonnull
param=strict:bool:optional:nonnull
"#,
    );
}
