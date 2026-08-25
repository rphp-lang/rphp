mod common;

use common::run_php;

#[test]
fn byte_utilities_preserve_php_bytes_modes_algorithms_limits_and_cow() {
    assert_eq!(
        run_php(
            r#"<?php
function bin(string $label, string $value): void {
    echo $label, '=', strlen($value), ':', bin2hex($value), "\n";
}
function sparse(array $counts): string {
    $parts = [];
    foreach ($counts as $byte => $count) $parts[] = "$byte:$count";
    return implode(',', $parts);
}

$value = "A.\\+*?[^]($)z\0\xffA";
bin('quote', quotemeta($value));
bin('rot', str_rot13($value));
echo 'counts0=', count(count_chars($value)), ':', count_chars($value)[65], ':', count_chars($value)[0], ':', count_chars($value)[255], "\n";
echo 'counts1=', sparse(count_chars($value, 1)), "\n";
echo 'counts2=', count(count_chars($value, 2)), ':', isset(count_chars($value, 2)[65]) ? 'bad' : 'absent', "\n";
bin('counts3', count_chars($value, 3));
echo 'counts4=', strlen(count_chars($value, 4)), "\n";

foreach (['', 'Robert', 'Ashcraft', 'Tymczak', "--\xffA\0Robert"] as $word) {
    bin('soundex/'.bin2hex($word), soundex($word));
}
foreach (['knight', 'xylophone', 'AMBER', 'CYA', 'SCIA', 'DDGE', 'DGYA', 'ghost', 'laugh', "A\0Robert"] as $word) {
    bin('meta/'.bin2hex($word), metaphone($word));
    bin('meta2/'.bin2hex($word), metaphone($word, 2));
}
bin('meta-x-limit', metaphone('AXA', 2));

$source = "\x80Az.\0";
$alias =& $source;
$rotated = str_rot13($source);
$quoted = quotemeta($source);
$used = count_chars($source, 3);
$source[1] = 'Q';
bin('cow-source', $source);
bin('cow-alias', $alias);
bin('cow-rotated', $rotated);
bin('cow-quoted', $quoted);
bin('cow-used', $used);

foreach ([-1, 5] as $mode) {
    try { count_chars('abc', $mode); }
    catch (Throwable $error) { echo 'mode=', $error::class, ':', $error->getMessage(), "\n"; }
}
try { metaphone('abc', -1); }
catch (Throwable $error) { echo 'limit=', $error::class, ':', $error->getMessage(), "\n"; }
"#,
        ),
        r#"quote=27:415c2e5c5c5c2b5c2a5c3f5c5b5c5e5c5d5c285c245c297a00ff41
rot=16:4e2e5c2b2a3f5b5e5d2824296d00ff4e
counts0=256:2:1:1
counts1=0:1,36:1,40:1,41:1,42:1,43:1,46:1,63:1,65:2,91:1,92:1,93:1,94:1,122:1,255:1
counts2=241:absent
counts3=15:002428292a2b2e3f415b5c5d5e7aff
counts4=241
soundex/=4:30303030
soundex/526f62657274=4:52313633
soundex/4173686372616674=4:41323236
soundex/54796d637a616b=4:54353232
soundex/2d2dff4100526f62657274=4:41363136
meta/6b6e69676874=3:4e4654
meta2/6b6e69676874=2:4e46
meta/78796c6f70686f6e65=4:534c464e
meta2/78796c6f70686f6e65=2:534c
meta/414d424552=3:414d52
meta2/414d424552=2:414d
meta/435941=2:5359
meta2/435941=2:5359
meta/53434941=2:5358
meta2/53434941=2:5358
meta/44444745=2:544a
meta2/44444745=2:544a
meta/44475941=2:4a59
meta2/44475941=2:4a59
meta/67686f7374=3:465354
meta2/67686f7374=2:4653
meta/6c61756768=2:4c46
meta2/6c61756768=2:4c46
meta/4100526f62657274=1:41
meta2/4100526f62657274=1:41
meta-x-limit=3:414b53
cow-source=5:80517a2e00
cow-alias=5:80517a2e00
cow-rotated=5:804e6d2e00
cow-quoted=6:80417a5c2e00
cow-used=5:002e417a80
mode=ValueError:count_chars(): Argument #2 ($mode) must be between 0 and 4 (inclusive)
mode=ValueError:count_chars(): Argument #2 ($mode) must be between 0 and 4 (inclusive)
limit=ValueError:metaphone(): Argument #2 ($max_phonemes) must be greater than or equal to 0
"#,
    );
}

#[test]
fn byte_utilities_own_weak_strict_stringable_and_diagnostic_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
function render(mixed $value): string {
    if (is_array($value)) return 'array:' . count($value);
    return 'string:' . strlen($value) . ':' . bin2hex($value);
}
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { echo render($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
final class TextValue {
    public function __toString(): string { echo "convert\n"; return 'Robert'; }
}
foreach (['quotemeta', 'soundex', 'str_rot13'] as $name) {
    attempt("$name/int", static fn () => $GLOBALS['name'](12.5));
    attempt("$name/null", static fn () => $GLOBALS['name'](null));
    attempt("$name/object", static fn () => $GLOBALS['name'](new TextValue()));
    attempt("$name/array", static fn () => $GLOBALS['name']([]));
}
attempt('count/int', static fn () => count_chars(12.5));
attempt('count/mode-string', static fn () => count_chars('abc', '3'));
attempt('count/mode-float', static fn () => count_chars('abc', 3.5));
attempt('count/mode-null', static fn () => count_chars('abc', null));
attempt('count/mode-array', static fn () => count_chars('abc', []));
attempt('meta/int', static fn () => metaphone(12.5));
attempt('meta/limit-string', static fn () => metaphone('testing', '2'));
attempt('meta/limit-float', static fn () => metaphone('testing', 2.5));
attempt('meta/limit-null', static fn () => metaphone('testing', null));
attempt('meta/limit-array', static fn () => metaphone('testing', []));
restore_error_handler();
"#,
        ),
        r#"quotemeta/int=string:5:31325c2e35
quotemeta/null=diag=8192:quotemeta(): Passing null to parameter #1 ($string) of type string is deprecated
string:0:
quotemeta/object=convert
string:6:526f62657274
quotemeta/array=TypeError:quotemeta(): Argument #1 ($string) must be of type string, array given
soundex/int=string:4:30303030
soundex/null=diag=8192:soundex(): Passing null to parameter #1 ($string) of type string is deprecated
string:4:30303030
soundex/object=convert
string:4:52313633
soundex/array=TypeError:soundex(): Argument #1 ($string) must be of type string, array given
str_rot13/int=string:4:31322e35
str_rot13/null=diag=8192:str_rot13(): Passing null to parameter #1 ($string) of type string is deprecated
string:0:
str_rot13/object=convert
string:6:45626f726567
str_rot13/array=TypeError:str_rot13(): Argument #1 ($string) must be of type string, array given
count/int=array:256
count/mode-string=string:3:616263
count/mode-float=diag=8192:Implicit conversion from float 3.5 to int loses precision
string:3:616263
count/mode-null=diag=8192:count_chars(): Passing null to parameter #2 ($mode) of type int is deprecated
array:256
count/mode-array=TypeError:count_chars(): Argument #2 ($mode) must be of type int, array given
meta/int=string:0:
meta/limit-string=string:2:5453
meta/limit-float=diag=8192:Implicit conversion from float 2.5 to int loses precision
string:2:5453
meta/limit-null=diag=8192:metaphone(): Passing null to parameter #2 ($max_phonemes) of type int is deprecated
string:5:5453544e4b
meta/limit-array=TypeError:metaphone(): Argument #2 ($max_phonemes) must be of type int, array given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo is_array($value) ? 'array:' . count($value) : 'string:' . bin2hex($value);
    } catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
attempt('quotemeta/int', static fn () => quotemeta(1));
attempt('soundex/int', static fn () => soundex(1));
attempt('rot/int', static fn () => str_rot13(1));
attempt('count/string', static fn () => count_chars('abc', 3));
attempt('count/path-int', static fn () => count_chars(1));
attempt('count/mode-string', static fn () => count_chars('abc', '3'));
attempt('meta/string', static fn () => metaphone('testing', 2));
attempt('meta/path-int', static fn () => metaphone(1));
attempt('meta/limit-string', static fn () => metaphone('testing', '2'));
"#,
        ),
        r#"quotemeta/int=TypeError:quotemeta(): Argument #1 ($string) must be of type string, int given
soundex/int=TypeError:soundex(): Argument #1 ($string) must be of type string, int given
rot/int=TypeError:str_rot13(): Argument #1 ($string) must be of type string, int given
count/string=string:616263
count/path-int=TypeError:count_chars(): Argument #1 ($string) must be of type string, int given
count/mode-string=TypeError:count_chars(): Argument #2 ($mode) must be of type int, string given
meta/string=string:5453
meta/path-int=TypeError:metaphone(): Argument #1 ($string) must be of type string, int given
meta/limit-string=TypeError:metaphone(): Argument #2 ($max_phonemes) must be of type int, string given
"#,
    );
}

#[test]
fn byte_utilities_share_named_dynamic_callback_reflection_and_order_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
function show(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo is_array($value) ? implode('', array_keys($value)) : bin2hex($value);
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
function mark(string $label, mixed $value): mixed { echo $label, '>'; return $value; }
final class OrderedText {
    public function __toString(): string { echo 'convert>'; return 'testing'; }
}

$dynamic = 'str_rot13';
$first = metaphone(...);
show('named', static fn () => count_chars(mode: 3, string: 'caba'));
show('dynamic', static fn () => ($GLOBALS['dynamic'])('Abm-Zn'));
show('first-class', static fn () => ($GLOBALS['first'])('xylophone', 3));
show('call-user', static fn () => call_user_func('soundex', 'Rupert'));
show('call-array', static fn () => call_user_func_array('quotemeta', ['string' => '.+']));
show('too-many', static fn () => soundex('a', 1));
show('unknown', static fn () => str_rot13(string: 'abc', extra: 1));
show('order', static fn () => count_chars(mark('input', new OrderedText()), mark('mode', new stdClass())));

foreach (['count_chars', 'metaphone', 'quotemeta', 'soundex', 'str_rot13'] as $name) {
    $function = new ReflectionFunction($name);
    echo 'reflection=', $name, ':', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', $function->getReturnType(), "\n";
    foreach ($function->getParameters() as $parameter) {
        echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
            $parameter->isOptional() ? 'optional' : 'required', ':',
            $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
    }
}
"#,
        ),
        r#"named=616263
dynamic=4e6f7a2d4d61
first-class=534c46
call-user=52313633
call-array=5c2e5c2b
too-many=ArgumentCountError:soundex() expects exactly 1 argument, 2 given
unknown=Error:Unknown named parameter $extra
order=input>mode>convert>TypeError:count_chars(): Argument #2 ($mode) must be of type int, stdClass given
reflection=count_chars:1/2:array|string
param=string:string:required:nonnull
param=mode:int:optional:nonnull
reflection=metaphone:1/2:string
param=string:string:required:nonnull
param=max_phonemes:int:optional:nonnull
reflection=quotemeta:1/1:string
param=string:string:required:nonnull
reflection=soundex:1/1:string
param=string:string:required:nonnull
reflection=str_rot13:1/1:string
param=string:string:required:nonnull
"#,
    );
}
