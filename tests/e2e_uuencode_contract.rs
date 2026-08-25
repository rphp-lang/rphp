mod common;

use common::run_php;

#[test]
fn uuencode_matches_php_bytes_lines_and_roundtrips() {
    assert_eq!(
        run_php(
            r#"<?php
function recordCase(string $label, string $input): void {
    $encoded = convert_uuencode($input);
    $decoded = convert_uudecode($encoded);
    echo $label, '=', strlen($input), ':', strlen($encoded), ':',
        substr_count($encoded, "\n"), ':', sha1($encoded), ':',
        $decoded === $input ? 'same' : 'different';
    if (strlen($input) <= 5) {
        echo ':', bin2hex($encoded), ':', bin2hex($decoded);
    }
    echo "\n";
}

$allBytes = '';
for ($byte = 0; $byte < 256; $byte++) {
    $allBytes .= chr($byte);
}
foreach ([
    'empty' => '',
    'one' => 'Q',
    'two' => 'QR',
    'three' => 'QRS',
    'binary' => "\0\x80\xff\r\n",
    'utf8' => "\xc4\x85\xe2\x82\xac",
    '44' => str_repeat('x', 44),
    '45' => str_repeat('y', 45),
    '46' => str_repeat('z', 46),
    '91' => str_repeat('c', 91),
    'all' => $allBytes,
] as $label => $input) {
    recordCase($label, $input);
}

$source = "A\0\xff";
$encoded = convert_uuencode($source);
$decoded = convert_uudecode($encoded);
$copy = $decoded;
$alias =& $decoded;
$decoded[0] = 'Z';
echo 'cow=', bin2hex($source), ':', bin2hex($encoded), ':',
    bin2hex($decoded), ':', bin2hex($copy), ':', bin2hex($alias), "\n";
"#,
        ),
        r#"empty=0:2:1:f6460d7d9d2b32d0dbd200d75a696a0a3e3a09e1:same:600a:
one=1:8:2:fd1559d8139d9246eda42c14d739d49066711f73:same:21343060600a600a:51
two=2:8:2:58db73d83c254f138c423e4c970df474ff34dfb1:same:22343528600a600a:5152
three=3:8:2:4b58169169a80f88a12004fcf147b238b082fa0b:same:23343529330a600a:515253
binary=5:12:2:cab8bffb919f917fef0c8bed0f59ad38f0533278:same:256028235f233048600a600a:0080ff0d0a
utf8=5:12:2:b7df06e9c2dfc8924bed1f0e6b0e0b9a2f3612f7:same:2551283742404a50600a600a:c485e282ac
44=44:64:2:2f64d7413049d97d1f94ee4b72dd36995c245f18:same
45=45:64:2:ade74c0c614155ac0dbffdf096c79e49bcf646f4:same
46=46:70:3:d739e3dcc039bfc3db7b2ca3cf0da5fdf01e5f72:same
91=91:132:4:2a042a96af41f47c584cf8d920280778654e2e98:same
all=256:358:7:7b3ac88460880e38c9864ba9380f0c743f2b4712:same
cow=4100ff:233030235f0a600a:5a00ff:4100ff:5a00ff
"#,
    );
}

#[test]
fn uudecode_owns_malformed_weak_and_strict_boundaries() {
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
        echo $value === false ? 'false' : strlen($value) . ':' . bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
final class UuText {
    public function __construct(private string $value) {}
    public function __toString(): string { echo "convert\n"; return $this->value; }
}

attempt('zero-backtick', static fn () => convert_uudecode("`\nignored"));
attempt('zero-space', static fn () => convert_uudecode(" \r\n"));
attempt('one-raw-nul', static fn () => convert_uudecode("!\0```\n`\n"));
attempt('short-trailing', static fn () => convert_uudecode("!````\nignored"));
attempt('empty-invalid', static fn () => convert_uudecode(''));
attempt('truncated-invalid', static fn () => convert_uudecode('"``'));
$full = 'M' . str_repeat('`', 60) . "\r\n";
attempt('full-crlf-invalid', static fn () => convert_uudecode($GLOBALS['full']));

attempt('encode-null', static fn () => convert_uuencode(null));
attempt('encode-true', static fn () => convert_uuencode(true));
attempt('encode-object', static fn () => convert_uuencode(new UuText('AZ')));
attempt('encode-array', static fn () => convert_uuencode([]));
attempt('decode-null', static fn () => convert_uudecode(null));
attempt('decode-object', static fn () => convert_uudecode(new UuText("`\n")));
attempt('decode-array', static fn () => convert_uudecode([]));
set_error_handler(static function (): never { throw new RuntimeException('warning-stop'); });
attempt('decode-warning-throws', static fn () => convert_uudecode(''));
"#,
        ),
        r#"zero-backtick=0:
zero-space=0:
one-raw-nul=1:80
short-trailing=1:00
empty-invalid=diag=2:convert_uudecode(): Argument #1 ($data) is not a valid uuencoded string
false
truncated-invalid=diag=2:convert_uudecode(): Argument #1 ($data) is not a valid uuencoded string
false
full-crlf-invalid=diag=2:convert_uudecode(): Argument #1 ($data) is not a valid uuencoded string
false
encode-null=diag=8192:convert_uuencode(): Passing null to parameter #1 ($string) of type string is deprecated
2:600a
encode-true=8:212c3060600a600a
encode-object=convert
8:22303548600a600a
encode-array=TypeError:convert_uuencode(): Argument #1 ($string) must be of type string, array given
decode-null=diag=8192:convert_uudecode(): Passing null to parameter #1 ($string) of type string is deprecated
diag=2:convert_uudecode(): Argument #1 ($data) is not a valid uuencoded string
false
decode-object=convert
0:
decode-array=TypeError:convert_uudecode(): Argument #1 ($string) must be of type string, array given
decode-warning-throws=RuntimeException:warning-stop
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo $value === false ? 'false' : strlen($value) . ':' . bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
attempt('encode-string', static fn () => convert_uuencode('A'));
attempt('encode-int', static fn () => convert_uuencode(120));
attempt('encode-bool', static fn () => convert_uuencode(true));
attempt('decode-string', static fn () => convert_uudecode("!00``\n`\n"));
attempt('decode-int', static fn () => convert_uudecode(120));
attempt('decode-null', static fn () => convert_uudecode(null));
"#,
        ),
        r#"encode-string=8:21303060600a600a
encode-int=TypeError:convert_uuencode(): Argument #1 ($string) must be of type string, int given
encode-bool=TypeError:convert_uuencode(): Argument #1 ($string) must be of type string, true given
decode-string=1:41
decode-int=TypeError:convert_uudecode(): Argument #1 ($string) must be of type string, int given
decode-null=TypeError:convert_uudecode(): Argument #1 ($string) must be of type string, null given
"#,
    );
}

#[test]
fn uuencode_call_shapes_and_reflection_share_one_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo $value === false ? 'false' : strlen($value) . ':' . bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}

foreach (['convert_uuencode', 'convert_uudecode'] as $name) {
    echo "function=$name\n";
    $input = $name === 'convert_uuencode' ? 'AZ' : "\"05H`\n`\n";
    $callback = $name(...);
    attempt('named', static fn () => $GLOBALS['name'](string: $GLOBALS['input']));
    attempt('dynamic', static fn () => ($GLOBALS['name'])($GLOBALS['input']));
    attempt('callback', static fn () => ($GLOBALS['callback'])($GLOBALS['input']));
    attempt('call-user', static fn () => call_user_func($GLOBALS['name'], $GLOBALS['input']));
    attempt('call-array', static fn () => call_user_func_array($GLOBALS['name'], ['string' => $GLOBALS['input']]));
    attempt('missing', static fn () => $GLOBALS['name']());
    attempt('too-many', static fn () => $GLOBALS['name']($GLOBALS['input'], 1));
    attempt('unknown', static fn () => $GLOBALS['name'](other: $GLOBALS['input']));
    $reflection = new ReflectionFunction($name);
    $parameter = $reflection->getParameters()[0];
    echo 'reflection=', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), ':',
        $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
}

$source = "A\0\xff";
$alias =& $source;
$copy = $source;
echo 'refs=', bin2hex(convert_uudecode(convert_uuencode($source))), ':',
    bin2hex($source), ':', bin2hex($alias), ':', bin2hex($copy), "\n";
"#,
        ),
        r#"function=convert_uuencode
named=8:22303548600a600a
dynamic=8:22303548600a600a
callback=8:22303548600a600a
call-user=8:22303548600a600a
call-array=8:22303548600a600a
missing=ArgumentCountError:convert_uuencode() expects exactly 1 argument, 0 given
too-many=ArgumentCountError:convert_uuencode() expects exactly 1 argument, 2 given
unknown=Error:Unknown named parameter $other
reflection=1/1:string:string:string:nonnull
function=convert_uudecode
named=2:415a
dynamic=2:415a
callback=2:415a
call-user=2:415a
call-array=2:415a
missing=ArgumentCountError:convert_uudecode() expects exactly 1 argument, 0 given
too-many=ArgumentCountError:convert_uudecode() expects exactly 1 argument, 2 given
unknown=Error:Unknown named parameter $other
reflection=1/1:string|false:string:string:nonnull
refs=4100ff:4100ff:4100ff:4100ff
"#,
    );
}
