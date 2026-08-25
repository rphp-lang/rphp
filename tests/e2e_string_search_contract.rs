mod common;

use common::run_php;

#[test]
fn string_search_and_windows_use_php_bytes_without_mutating_sources() {
    assert_eq!(
        run_php(
            r#"<?php
function dump_value(string $label, mixed $value): void {
    echo $label, '=';
    if (is_string($value)) { echo 's:', bin2hex($value), "\n"; }
    else { var_dump($value); }
}
$binary = "A\0" . chr(128) . "é" . chr(255) . 'A';
dump_value('binary', $binary);
dump_value('strpos-high', strpos($binary, chr(128)));
dump_value('strpos-utf8', strpos($binary, "é"));
dump_value('strpos-negative', strpos($binary, 'A', -1));
dump_value('strstr-suffix', strstr($binary, "é"));
dump_value('strstr-prefix', strstr($binary, "é", true));
dump_value('strrchr-suffix', strrchr($binary, 'A'));
dump_value('strrchr-prefix', strrchr($binary, 'A', true));
dump_value('strrchr-empty', strrchr("a\0b", ''));
dump_value('strspn-high', strspn(chr(128) . chr(128) . 'A', chr(128)));
dump_value('strcspn-high', strcspn(chr(128) . chr(255) . 'A', 'A'));
dump_value('strspn-window', strspn('aabbcc', 'ab', -5, -1));
dump_value('strcspn-window', strcspn('aabbcc', 'c', 2, null));
dump_value('count-window', substr_count('ababa', 'a', -4, -1));
dump_value('count-zero', substr_count('ababa', 'a', 5, 0));
dump_value('compare-zero-invalid-offset', substr_compare('abc', 'z', 99, 0));
dump_value('compare-binary', substr_compare($binary, chr(128), 2, 1));
dump_value('compare-fold', substr_compare('aBc', 'AB', 0, 2, true));

$source = $binary;
$alias =& $source;
dump_value('cow-search', strstr($source, chr(128)));
dump_value('cow-source', $source);
dump_value('cow-alias', $alias);
"#,
        ),
        r#"binary=s:410080c3a9ff41
strpos-high=int(2)
strpos-utf8=int(3)
strpos-negative=int(6)
strstr-suffix=s:c3a9ff41
strstr-prefix=s:410080
strrchr-suffix=s:41
strrchr-prefix=s:410080c3a9ff
strrchr-empty=s:0062
strspn-high=int(2)
strcspn-high=int(2)
strspn-window=int(3)
strcspn-window=int(2)
count-window=int(1)
count-zero=int(0)
compare-zero-invalid-offset=int(0)
compare-binary=int(0)
compare-fold=int(0)
cow-search=s:80c3a9ff41
cow-source=s:410080c3a9ff41
cow-alias=s:410080c3a9ff41
"#,
    );
}

#[test]
fn string_search_boundaries_match_weak_and_strict_php_types() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
function dump_value(mixed $value): void {
    if (is_string($value)) { echo 's:', bin2hex($value), "\n"; }
    else { var_dump($value); }
}
function attempt(string $label, callable $call): void {
    echo '[', $label, "]\n";
    try { dump_value($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
attempt('null-haystack', static fn () => strpos(null, ''));
attempt('numeric-offset', static fn () => strpos('abc', 'b', '1'));
attempt('bad-offset', static fn () => strpos('abc', 'b', 'x'));
attempt('empty-needle-count', static fn () => substr_count('abc', '', 99));
attempt('null-length-count', static fn () => substr_count('ababa', 'a', 1, null));
attempt('bad-span-offset', static fn () => strspn('abc', 'a', []));
attempt('negative-compare-length', static fn () => substr_compare('abc', 'a', 99, -1));
attempt('null-before-strrchr', static fn () => strrchr("a\0b", '', null));
attempt('array-haystack', static fn () => strstr([], 'a'));
restore_error_handler();
"#,
        ),
        r#"[null-haystack]
diag=8192:strpos(): Passing null to parameter #1 ($haystack) of type string is deprecated
int(0)
[numeric-offset]
int(1)
[bad-offset]
TypeError:strpos(): Argument #3 ($offset) must be of type int, string given
[empty-needle-count]
ValueError:substr_count(): Argument #2 ($needle) must not be empty
[null-length-count]
int(2)
[bad-span-offset]
TypeError:strspn(): Argument #3 ($offset) must be of type int, array given
[negative-compare-length]
ValueError:substr_compare(): Argument #4 ($length) must be greater than or equal to 0
[null-before-strrchr]
diag=8192:strrchr(): Passing null to parameter #3 ($before_needle) of type bool is deprecated
s:0062
[array-haystack]
TypeError:strstr(): Argument #1 ($haystack) must be of type string, array given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo '[', $label, "]\n";
    try { var_dump($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
attempt('strpos-haystack', static fn () => strpos(123, '2'));
attempt('strpos-needle', static fn () => strpos('123', 2));
attempt('strpos-offset', static fn () => strpos('abc', 'b', '1'));
attempt('strstr-before', static fn () => strstr('abc', 'b', 1));
attempt('strrchr-before', static fn () => strrchr('abc', 'b', 1));
attempt('span-characters', static fn () => strspn('123', 12));
attempt('span-length', static fn () => strcspn('abc', 'z', 0, '2'));
attempt('count-offset', static fn () => substr_count('abc', 'a', true));
attempt('count-length-null', static fn () => substr_count('ababa', 'a', 0, null));
attempt('compare-case', static fn () => substr_compare('a', 'A', 0, null, 1));
"#,
        ),
        r#"[strpos-haystack]
TypeError:strpos(): Argument #1 ($haystack) must be of type string, int given
[strpos-needle]
TypeError:strpos(): Argument #2 ($needle) must be of type string, int given
[strpos-offset]
TypeError:strpos(): Argument #3 ($offset) must be of type int, string given
[strstr-before]
TypeError:strstr(): Argument #3 ($before_needle) must be of type bool, int given
[strrchr-before]
TypeError:strrchr(): Argument #3 ($before_needle) must be of type bool, int given
[span-characters]
TypeError:strspn(): Argument #2 ($characters) must be of type string, int given
[span-length]
TypeError:strcspn(): Argument #4 ($length) must be of type ?int, string given
[count-offset]
TypeError:substr_count(): Argument #3 ($offset) must be of type int, true given
[count-length-null]
int(3)
[compare-case]
TypeError:substr_compare(): Argument #5 ($case_insensitive) must be of type bool, int given
"#,
    );
}

#[test]
fn string_search_call_shapes_and_reflection_share_one_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function dump_value(string $label, mixed $value): void {
    echo $label, '=';
    if (is_string($value)) { echo 's:', bin2hex($value), "\n"; }
    else { var_dump($value); }
}
function attempt(string $label, callable $call): void {
    echo '[', $label, "]\n";
    try { var_dump($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
$dynamic = 'substr_count';
dump_value('dynamic', $dynamic('ababa', 'a', 1, 3));
dump_value('named', substr_compare(case_insensitive: true, needle: 'AB', haystack: 'aBc', offset: 0, length: 2));
dump_value('callback', call_user_func('strrchr', 'abcabc', 'b', true));
dump_value('callback-named', call_user_func_array('strspn', [
    'characters' => 'ab', 'string' => 'aabc', 'length' => 3, 'offset' => 0,
]));
attempt('too-few', static fn () => strpos('a'));
attempt('too-many', static fn () => strrchr('a', 'a', false, 'ignored'));
attempt('unknown-named', static fn () => substr_count(haystack: 'a', needle: 'a', extra: 1));

foreach (['strpos', 'strstr', 'strrchr', 'strspn', 'strcspn', 'substr_count', 'substr_compare'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo 'reflection=', $name, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
    foreach ($reflection->getParameters() as $parameter) {
        echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
            $parameter->isOptional() ? 'optional' : 'required', ':',
            $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
    }
}
"#,
        ),
        r#"dynamic=int(1)
named=int(0)
callback=s:61626361
callback-named=int(3)
[too-few]
ArgumentCountError:strpos() expects at least 2 arguments, 1 given
[too-many]
ArgumentCountError:strrchr() expects at most 3 arguments, 4 given
[unknown-named]
Error:Unknown named parameter $extra
reflection=strpos:2/3:int|false
param=haystack:string:required:nonnull
param=needle:string:required:nonnull
param=offset:int:optional:nonnull
reflection=strstr:2/3:string|false
param=haystack:string:required:nonnull
param=needle:string:required:nonnull
param=before_needle:bool:optional:nonnull
reflection=strrchr:2/3:string|false
param=haystack:string:required:nonnull
param=needle:string:required:nonnull
param=before_needle:bool:optional:nonnull
reflection=strspn:2/4:int
param=string:string:required:nonnull
param=characters:string:required:nonnull
param=offset:int:optional:nonnull
param=length:?int:optional:nullable
reflection=strcspn:2/4:int
param=string:string:required:nonnull
param=characters:string:required:nonnull
param=offset:int:optional:nonnull
param=length:?int:optional:nullable
reflection=substr_count:2/4:int
param=haystack:string:required:nonnull
param=needle:string:required:nonnull
param=offset:int:optional:nonnull
param=length:?int:optional:nullable
reflection=substr_compare:3/5:int
param=haystack:string:required:nonnull
param=needle:string:required:nonnull
param=offset:int:required:nonnull
param=length:?int:optional:nullable
param=case_insensitive:bool:optional:nonnull
"#,
    );
}
