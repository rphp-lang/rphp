mod common;

use common::run_php;

#[test]
fn str_getcsv_parses_php_bytes_multiline_fields_and_control_collisions() {
    assert_eq!(
        run_php(
            r#"<?php
function row(string $label, array $fields): void {
    echo $label, '=', count($fields);
    foreach ($fields as $field) {
        echo ':';
        if ($field === null) echo 'N';
        else echo strlen($field), '/', bin2hex($field);
    }
    echo "\n";
}

row('blank', str_getcsv('', escape: ''));
row('empty-columns', str_getcsv(',,tail,', escape: ''));
row('multiline', str_getcsv("\"north\nsouth\",mid\nline,end\r\n", escape: ''));
row('leading', str_getcsv(" \t~left;right~tail; x ", ';', '~', ''));
row('same-control', str_getcsv('.red..blue.', '.', '.', '.'));
row('high-separator', str_getcsv("\xffA\xffB\xff", "\xff", '"', ''));
row('high-escape', str_getcsv("\x22\x61\xfe\x22\x62\x22\x2c\x63", ',', '"', "\xfe"));
row('nul-open', str_getcsv("\0yy", 'y', 'y', "\0"));
row('utf8', str_getcsv("é,β", escape: ''));

$source = "\x80;z";
$alias =& $source;
$parsed = str_getcsv($source, ';', '"', '');
$source[0] = 'Q';
row('detached', $parsed);
echo 'source=', bin2hex($source), ':alias=', bin2hex($alias), "\n";
"#,
        ),
        r#"blank=1:N
empty-columns=4:0/:0/:4/7461696c:0/
multiline=3:11/6e6f7274680a736f757468:8/6d69640a6c696e65:3/656e64
leading=2:14/6c6566743b72696768747461696c:3/207820
same-control=1:8/7265642e626c7565
high-separator=4:0/:1/41:1/42:0/
high-escape=2:4/61fe6222:1/63
nul-open=2:1/00:1/00
utf8=2:2/c3a9:2/ceb2
detached=2:1/80:1/7a
source=513b7a:alias=513b7a
"#,
    );
}

#[test]
fn str_getcsv_owns_weak_strict_and_diagnostic_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
function attempt(string $label, callable $call): void {
    echo '[', $label, "]\n";
    try {
        $fields = $call();
        echo count($fields);
        foreach ($fields as $field) echo ':', $field === null ? 'N' : bin2hex($field);
        echo "\n";
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage(), "\n";
    }
}
class CsvText { public function __toString(): string { echo "convert\n"; return 'u:v'; } }
attempt('source-int', static fn () => str_getcsv(120, escape: ''));
attempt('source-null', static fn () => str_getcsv(null, escape: ''));
attempt('source-object', static fn () => str_getcsv(new CsvText(), ':', '"', ''));
attempt('source-array', static fn () => str_getcsv([], escape: ''));
attempt('separator-true', static fn () => str_getcsv('a1b', true, '"', ''));
attempt('separator-int', static fn () => str_getcsv('a,b', 44, '"', ''));
attempt('enclosure-false', static fn () => str_getcsv('a,b', ',', false, ''));
attempt('escape-null', static fn () => str_getcsv('a,b', ',', '"', null));
attempt('escape-wide', static fn () => str_getcsv('a,b', ',', '"', 'xx'));
attempt('default-escape', static fn () => str_getcsv('a,b'));
restore_error_handler();
"#,
        ),
        r#"[source-int]
1:313230
[source-null]
diag=8192:str_getcsv(): Passing null to parameter #1 ($string) of type string is deprecated
1:N
[source-object]
convert
2:75:76
[source-array]
TypeError:str_getcsv(): Argument #1 ($string) must be of type string, array given
[separator-true]
2:61:62
[separator-int]
ValueError:str_getcsv(): Argument #2 ($separator) must be a single character
[enclosure-false]
ValueError:str_getcsv(): Argument #3 ($enclosure) must be a single character
[escape-null]
diag=8192:str_getcsv(): Passing null to parameter #4 ($escape) of type string is deprecated
2:61:62
[escape-wide]
ValueError:str_getcsv(): Argument #4 ($escape) must be empty or a single character
[default-escape]
diag=8192:str_getcsv(): the $escape parameter must be provided as its default value will change
2:61:62
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo '[', $label, "]\n";
    try { echo count($call()), "\n"; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
attempt('valid', static fn () => str_getcsv('a,b', escape: ''));
attempt('source', static fn () => str_getcsv(12, escape: ''));
attempt('separator', static fn () => str_getcsv('a,b', 44, '"', ''));
attempt('enclosure', static fn () => str_getcsv('a,b', ',', true, ''));
attempt('escape', static fn () => str_getcsv('a,b', ',', '"', false));
"#,
        ),
        r#"[valid]
2
[source]
TypeError:str_getcsv(): Argument #1 ($string) must be of type string, int given
[separator]
TypeError:str_getcsv(): Argument #2 ($separator) must be of type string, int given
[enclosure]
TypeError:str_getcsv(): Argument #3 ($enclosure) must be of type string, true given
[escape]
TypeError:str_getcsv(): Argument #4 ($escape) must be of type string, false given
"#,
    );
}

#[test]
fn str_getcsv_call_shapes_and_reflection_share_the_typed_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function fields(array $row): string {
    $parts = [];
    foreach ($row as $field) $parts[] = $field === null ? 'N' : bin2hex($field);
    return implode('/', $parts);
}
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { echo fields($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
$name = 'str_getcsv';
$callback = str_getcsv(...);
attempt('named', static fn () => str_getcsv(string: 'a;b', separator: ';', escape: ''));
attempt('dynamic', static fn () => $GLOBALS['name']('a;b', ';', '"', ''));
attempt('callback', static fn () => ($GLOBALS['callback'])('a;b', ';', '"', ''));
attempt('call-user', static fn () => call_user_func('str_getcsv', 'a;b', ';', '"', ''));
attempt('call-array', static fn () => call_user_func_array('str_getcsv', ['escape' => '', 'string' => 'a;b', 'separator' => ';']));
attempt('too-many', static fn () => str_getcsv('a', ',', '"', '', 5));
attempt('unknown', static fn () => str_getcsv(string: 'a', extra: ''));

$reflection = new ReflectionFunction('str_getcsv');
echo 'reflection=', $reflection->getNumberOfRequiredParameters(), '/',
    $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
foreach ($reflection->getParameters() as $parameter) {
    echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isOptional() ? 'optional' : 'required', ':',
        $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
}
"#,
        ),
        r#"named=61/62
dynamic=61/62
callback=61/62
call-user=61/62
call-array=61/62
too-many=ArgumentCountError:str_getcsv() expects at most 4 arguments, 5 given
unknown=Error:Unknown named parameter $extra
reflection=1/4:array
param=string:string:required:nonnull
param=separator:string:optional:nonnull
param=enclosure:string:optional:nonnull
param=escape:string:optional:nonnull
"#,
    );
}
