mod common;

use common::run_php;

#[test]
fn legacy_utf8_conversions_are_byte_exact_and_preserve_cow() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(0);
function show(string $label, string $value): void {
    echo $label, '=', strlen($value), ':', bin2hex($value), "\n";
}

show('encode/latin1', utf8_encode("A\0\x7f\x80\xbf\xc0\xff"));
show('encode/source-utf8', utf8_encode('žluťoučký'));
$inputs = [
    'empty' => '',
    'latin1-range' => "\xc2\x80\xc2\xbf\xc3\x80\xc3\xbf",
    'above-latin1' => "\xc4\x80\xdf\xbf\xe0\xa0\x80\xef\xbf\xbf\xf0\x90\x80\x80\xf4\x8f\xbf\xbf",
    'overlong' => "\xc0\x80\xc1\xbf\xe0\x80\x80\xf0\x80\x80\x80",
    'surrogate-range' => "\xed\x9f\xbf\xed\xa0\x80\xed\xbf\xbf\xee\x80\x80",
    'out-of-range' => "\xf4\x8f\xbf\xbf\xf4\x90\x80\x80\xf5\x80\x80\x80\xff",
    'truncated' => "\xc2|\xc2\x80|\xe0|\xe0\xa0|\xe0\xa0\x80|\xf0|\xf0\x90|\xf0\x90\x80|\xf0\x90\x80\x80",
    'restart' => "\xe0\xa0A\xe0\xa0\xc2\x80\xf0\x90\x80\xffB\x80",
];
foreach ($inputs as $label => $input) show('decode/' . $label, utf8_decode($input));

$value = "A\xc2\x80\xe0\xa0\x80\xffZ";
$alias =& $value;
$decoded = utf8_decode($value);
$encoded = utf8_encode($decoded);
$value[0] = 'X';
show('cow/source', $value);
show('cow/alias', $alias);
show('cow/decoded', $decoded);
show('cow/encoded', $encoded);
"#,
        ),
        r#"encode/latin1=11:41007fc280c2bfc380c3bf
encode/source-utf8=21:c385c2be6c75c385c2a56f75c384c28d6bc383c2bd
decode/empty=0:
decode/latin1-range=4:80bfc0ff
decode/above-latin1=6:3f3f3f3f3f3f
decode/overlong=6:3f3f3f3f3f3f
decode/surrogate-range=4:3f3f3f3f
decode/out-of-range=7:3f3f3f3f3f3f3f
decode/truncated=17:3f7c807c3f7c3f7c3f7c3f7c3f7c3f7c3f
decode/restart=7:3f413f803f423f
cow/source=8:58c280e0a080ff5a
cow/alias=8:58c280e0a080ff5a
cow/decoded=5:41803f3f5a
cow/encoded=6:41c2803f3f5a
"#,
    );
}

#[test]
fn legacy_utf8_conversions_own_weak_strict_and_deprecation_order() {
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
        echo get_debug_type($value), ':', is_string($value) ? bin2hex($value) : (string) $value;
    } catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
final class TextValue {
    public function __construct(private string $value) {}
    public function __toString(): string { echo "convert\n"; return $this->value; }
}
attempt('encode/int', static fn () => utf8_encode(255));
attempt('encode/null', static fn () => utf8_encode(null));
attempt('encode/object', static fn () => utf8_encode(new TextValue("\xff")));
attempt('encode/array', static fn () => utf8_encode([]));
attempt('encode/missing', static fn () => utf8_encode());
attempt('encode/extra', static fn () => utf8_encode('x', 'y'));
attempt('decode/int', static fn () => utf8_decode(255));
attempt('decode/null', static fn () => utf8_decode(null));
attempt('decode/object', static fn () => utf8_decode(new TextValue("\xc3\xbf")));
attempt('decode/array', static fn () => utf8_decode([]));
restore_error_handler();
"#,
        ),
        r#"encode/int=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
string:323535
encode/null=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
diag=8192:utf8_encode(): Passing null to parameter #1 ($string) of type string is deprecated
string:
encode/object=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
convert
string:c3bf
encode/array=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
TypeError:utf8_encode(): Argument #1 ($string) must be of type string, array given
encode/missing=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
ArgumentCountError:utf8_encode() expects exactly 1 argument, 0 given
encode/extra=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
ArgumentCountError:utf8_encode() expects exactly 1 argument, 2 given
decode/int=diag=8192:Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives
string:323535
decode/null=diag=8192:Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives
diag=8192:utf8_decode(): Passing null to parameter #1 ($string) of type string is deprecated
string:
decode/object=diag=8192:Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives
convert
string:ff
decode/array=diag=8192:Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives
TypeError:utf8_decode(): Argument #1 ($string) must be of type string, array given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { echo bin2hex($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
attempt('encode/valid', static fn () => utf8_encode("\xff"));
attempt('encode/int', static fn () => utf8_encode(1));
attempt('decode/valid', static fn () => utf8_decode("\xc3\xbf"));
attempt('decode/int', static fn () => utf8_decode(1));
restore_error_handler();
"#,
        ),
        r#"encode/valid=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
c3bf
encode/int=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
TypeError:utf8_encode(): Argument #1 ($string) must be of type string, int given
decode/valid=diag=8192:Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives
ff
decode/int=diag=8192:Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives
TypeError:utf8_decode(): Argument #1 ($string) must be of type string, int given
"#,
    );
}

#[test]
fn legacy_utf8_conversions_share_call_and_reflection_metadata() {
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
    try { echo bin2hex($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
function mark(string $label, mixed $value): mixed { echo $label, '>'; return $value; }
final class OrderedText {
    public function __toString(): string { echo 'convert>'; return "\xff"; }
}
$dynamic = 'utf8_decode';
$first = utf8_encode(...);
attempt('named', static fn () => utf8_encode(string: "\xff"));
attempt('dynamic', static fn () => ($GLOBALS['dynamic'])("\xc3\xbf"));
attempt('first-class', static fn () => ($GLOBALS['first'])("\xff"));
attempt('call-user', static fn () => call_user_func('utf8_decode', "\xc3\xbf"));
attempt('call-array', static fn () => call_user_func_array('utf8_encode', ['string' => "\xff"]));
attempt('unknown', static fn () => utf8_encode(mystery: 'x'));
attempt('order', static fn () => utf8_encode(mark('arg', new OrderedText())));
foreach (['utf8_encode', 'utf8_decode'] as $name) {
    $function = new ReflectionFunction($name);
    echo 'reflection=', $name, ':', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', $function->getReturnType(), ':',
        $function->isDeprecated() ? 'deprecated' : 'current', "\n";
    foreach ($function->getParameters() as $parameter) {
        echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
            $parameter->isOptional() ? 'optional' : 'required', ':',
            $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
    }
    foreach ($function->getAttributes() as $attribute) {
        echo 'attribute=', $attribute->getName(), ':', json_encode($attribute->getArguments()), "\n";
    }
}
restore_error_handler();
"#,
        ),
        r#"named=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
c3bf
dynamic=diag=8192:Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives
ff
first-class=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
c3bf
call-user=diag=8192:Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives
ff
call-array=diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
c3bf
unknown=Error:Unknown named parameter $mystery
order=arg>diag=8192:Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives
convert>c3bf
reflection=utf8_encode:1/1:string:deprecated
param=string:string:required:nonnull
attribute=Deprecated:{"since":"8.2","message":"visit the php.net documentation for various alternatives"}
reflection=utf8_decode:1/1:string:deprecated
param=string:string:required:nonnull
attribute=Deprecated:{"since":"8.2","message":"visit the php.net documentation for various alternatives"}
"#,
    );
}
