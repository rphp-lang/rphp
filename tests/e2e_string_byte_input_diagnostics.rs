mod common;

use common::run_php;

#[test]
fn byte_input_functions_validate_and_preserve_exact_bytes() {
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
        echo get_debug_type($value), ':';
        if (is_string($value)) echo strlen($value), ':', bin2hex($value);
        elseif (is_bool($value)) echo $value ? 'true' : 'false';
        else echo (string) $value;
    } catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
foreach ([
    ['empty', ''], ['zero', '00'], ['bytes', '000fff'], ['mixed-case', 'fF'],
    ['odd', '0'], ['invalid-left', 'g0'], ['invalid-right', '0g'],
    ['odd-invalid', 'g'], ['nul', "\0"],
] as [$label, $input]) attempt('hex/' . $label, static fn () => hex2bin($input));
foreach ([
    ['empty', ''], ['one', 'A'], ['two', 'AB'], ['nul', "\0"],
    ['high', chr(255)], ['utf8', 'ž'],
] as [$label, $input]) attempt('ord/' . $label, static fn () => ord($input));
attempt('pbrk/empty', static fn () => strpbrk('abc', ''));
attempt('pbrk/none', static fn () => strpbrk('abc', 'XYZ'));
attempt('pbrk/ascii', static fn () => strpbrk('abc', 'xb'));
attempt('pbrk/nul', static fn () => strpbrk("a\0b", "\0"));
attempt('pbrk/high', static fn () => strpbrk("a" . chr(255) . "b", chr(255)));
restore_error_handler();
"#,
        ),
        r#"hex/empty=string:0:
hex/zero=string:1:00
hex/bytes=string:3:000fff
hex/mixed-case=string:1:ff
hex/odd=diag=2:hex2bin(): Hexadecimal input string must have an even length
bool:false
hex/invalid-left=diag=2:hex2bin(): Input string must be hexadecimal string
bool:false
hex/invalid-right=diag=2:hex2bin(): Input string must be hexadecimal string
bool:false
hex/odd-invalid=diag=2:hex2bin(): Hexadecimal input string must have an even length
bool:false
hex/nul=diag=2:hex2bin(): Hexadecimal input string must have an even length
bool:false
ord/empty=diag=8192:ord(): Providing an empty string is deprecated
int:0
ord/one=int:65
ord/two=diag=8192:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
int:65
ord/nul=int:0
ord/high=int:255
ord/utf8=diag=8192:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
int:197
pbrk/empty=ValueError:strpbrk(): Argument #2 ($characters) must be a non-empty string
pbrk/none=bool:false
pbrk/ascii=string:2:6263
pbrk/nul=string:2:0062
pbrk/high=string:2:ff62
"#,
    );
}

#[test]
fn byte_input_functions_own_weak_strict_and_stringable_boundaries() {
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
        echo get_debug_type($value), ':';
        if (is_string($value)) echo bin2hex($value);
        elseif (is_bool($value)) echo $value ? 'true' : 'false';
        else echo (string) $value;
    } catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
final class TextValue {
    public function __construct(private string $value) {}
    public function __toString(): string { echo 'convert>'; return $this->value; }
}
attempt('hex/int', static fn () => hex2bin(255));
attempt('hex/null', static fn () => hex2bin(null));
attempt('hex/object', static fn () => hex2bin(new TextValue('41')));
attempt('hex/array', static fn () => hex2bin([]));
attempt('ord/int', static fn () => ord(255));
attempt('ord/null', static fn () => ord(null));
attempt('ord/object', static fn () => ord(new TextValue('BC')));
attempt('ord/array', static fn () => ord([]));
attempt('pbrk/scalars', static fn () => strpbrk(12345, 34));
attempt('pbrk/null-haystack', static fn () => strpbrk(null, 'a'));
attempt('pbrk/null-characters', static fn () => strpbrk('abc', null));
attempt('pbrk/object', static fn () => strpbrk(new TextValue('abc'), new TextValue('b')));
attempt('pbrk/array', static fn () => strpbrk([], 'a'));
restore_error_handler();
"#,
        ),
        r#"hex/int=diag=2:hex2bin(): Hexadecimal input string must have an even length
bool:false
hex/null=diag=8192:hex2bin(): Passing null to parameter #1 ($string) of type string is deprecated
string:
hex/object=convert>string:41
hex/array=TypeError:hex2bin(): Argument #1 ($string) must be of type string, array given
ord/int=diag=8192:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
int:50
ord/null=diag=8192:ord(): Passing null to parameter #1 ($character) of type string is deprecated
diag=8192:ord(): Providing an empty string is deprecated
int:0
ord/object=convert>diag=8192:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
int:66
ord/array=TypeError:ord(): Argument #1 ($character) must be of type string, array given
pbrk/scalars=string:333435
pbrk/null-haystack=diag=8192:strpbrk(): Passing null to parameter #1 ($string) of type string is deprecated
bool:false
pbrk/null-characters=diag=8192:strpbrk(): Passing null to parameter #2 ($characters) of type string is deprecated
ValueError:strpbrk(): Argument #2 ($characters) must be a non-empty string
pbrk/object=convert>convert>string:6263
pbrk/array=TypeError:strpbrk(): Argument #1 ($string) must be of type string, array given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo get_debug_type($value), ':', is_string($value) ? bin2hex($value) : (string) $value;
    } catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
attempt('hex/valid', static fn () => hex2bin('41'));
attempt('hex/int', static fn () => hex2bin(41));
attempt('ord/valid', static fn () => ord('A'));
attempt('ord/int', static fn () => ord(65));
attempt('pbrk/valid', static fn () => strpbrk('abc', 'b'));
attempt('pbrk/int1', static fn () => strpbrk(123, '2'));
attempt('pbrk/int2', static fn () => strpbrk('123', 2));
"#,
        ),
        r#"hex/valid=string:41
hex/int=TypeError:hex2bin(): Argument #1 ($string) must be of type string, int given
ord/valid=int:65
ord/int=TypeError:ord(): Argument #1 ($character) must be of type string, int given
pbrk/valid=string:6263
pbrk/int1=TypeError:strpbrk(): Argument #1 ($string) must be of type string, int given
pbrk/int2=TypeError:strpbrk(): Argument #2 ($characters) must be of type string, int given
"#,
    );
}

#[test]
fn byte_input_functions_share_calls_reflection_and_throwing_diagnostics() {
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
        echo get_debug_type($value), ':', is_string($value) ? bin2hex($value) : (is_bool($value) ? ($value ? 'true' : 'false') : (string) $value);
    } catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
function mark(string $label, mixed $value): mixed { echo $label, '>'; return $value; }
$dynamic = 'hex2bin';
$first = ord(...);
attempt('named', static fn () => hex2bin(string: mark('arg', 'g0')));
attempt('dynamic', static fn () => ($GLOBALS['dynamic'])('41'));
attempt('first', static fn () => ($GLOBALS['first'])('AB'));
attempt('call-user', static fn () => call_user_func('strpbrk', 'abc', ''));
attempt('call-user-ord', static fn () => call_user_func('ord', 'AB'));
attempt('call-array', static fn () => call_user_func_array('hex2bin', ['string' => '0']));
attempt('call-array-ord', static fn () => call_user_func_array('ord', ['character' => 'AB']));
attempt('unknown', static fn () => ord(mystery: 'A'));
attempt('missing', static fn () => strpbrk('abc'));
attempt('extra', static fn () => hex2bin('41', '42'));
$hexSource = '4142';
$hexAlias =& $hexSource;
$decoded = hex2bin($hexSource);
$decoded[0] = 'Z';
echo 'cow-hex=', bin2hex($hexSource), ':', bin2hex($hexAlias), ':', bin2hex($decoded), "\n";
$haystack = "a\0b";
$haystackAlias =& $haystack;
$suffix = strpbrk($haystack, "\0");
$suffix[0] = 'X';
echo 'cow-pbrk=', bin2hex($haystack), ':', bin2hex($haystackAlias), ':', bin2hex($suffix), "\n";
$ordSource = 'A';
$ordAlias =& $ordSource;
echo 'ref-ord=', ord($ordSource), ':';
$ordAlias = 'B';
echo ord($ordSource), "\n";
foreach (['hex2bin', 'ord', 'strpbrk'] as $name) {
    $function = new ReflectionFunction($name);
    echo 'reflection=', $name, ':', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', $function->getReturnType(), "\n";
    foreach ($function->getParameters() as $parameter) {
        echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
            $parameter->isOptional() ? 'optional' : 'required', ':',
            $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
    }
}
restore_error_handler();
"#,
        ),
        r#"named=arg>diag=2:hex2bin(): Input string must be hexadecimal string
bool:false
dynamic=string:41
first=diag=8192:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
int:65
call-user=ValueError:strpbrk(): Argument #2 ($characters) must be a non-empty string
call-user-ord=diag=8192:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
int:65
call-array=diag=2:hex2bin(): Hexadecimal input string must have an even length
bool:false
call-array-ord=diag=8192:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
int:65
unknown=Error:Unknown named parameter $mystery
missing=ArgumentCountError:strpbrk() expects exactly 2 arguments, 1 given
extra=ArgumentCountError:hex2bin() expects exactly 1 argument, 2 given
cow-hex=34313432:34313432:5a42
cow-pbrk=610062:610062:5862
ref-ord=65:66
reflection=hex2bin:1/1:string|false
param=string:string:required:nonnull
reflection=ord:1/1:int
param=character:string:required:nonnull
reflection=strpbrk:2/2:string|false
param=string:string:required:nonnull
param=characters:string:required:nonnull
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { $call(); echo 'returned'; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
set_error_handler(static function (int $level, string $message): never {
    throw new ErrorException($message, 0, $level);
});
attempt('hex', static fn () => hex2bin('g0'));
attempt('ord-direct', static fn () => ord('AB'));
attempt('ord-dynamic', static fn () => ('ord')('AB'));
attempt('ord-call-user', static fn () => call_user_func('ord', 'AB'));
restore_error_handler();
"#,
        ),
        r#"hex=ErrorException:hex2bin(): Input string must be hexadecimal string
ord-direct=ErrorException:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
ord-dynamic=ErrorException:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
ord-call-user=ErrorException:ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead
"#,
    );
}
