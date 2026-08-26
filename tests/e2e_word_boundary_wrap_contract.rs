mod common;

use common::run_php;

#[test]
fn word_boundary_and_wrap_are_byte_exact_and_preserve_cow() {
    assert_eq!(
        run_php(
            r#"<?php
function render_words(int|array $value): string {
    if (is_int($value)) return 'int:' . $value;
    $parts = [];
    foreach ($value as $key => $word) $parts[] = $key . ':' . bin2hex($word);
    return 'array:' . implode(',', $parts);
}
function show_words(string $label, string $input, int $format, ?string $characters = null): void {
    echo $label, '=', render_words(str_word_count($input, $format, $characters)), "\n";
}
function show_wrap(string $label, string $input, int $width, string $break, bool $cut): void {
    echo $label, '=', strlen(wordwrap($input, $width, $break, $cut)), ':',
        bin2hex(wordwrap($input, $width, $break, $cut)), "\n";
}

$sample = "'one can't tail- a-b 12_z.\0\xffA";
show_words('count', $sample, 0);
show_words('list', $sample, 1);
show_words('offsets', $sample, 2);
show_words('digits', $sample, 2, '0..9_');
show_words('all-bytes', $sample, 2, "\0..\xff");
show_words('utf8-c-locale', 'žluťoučký kůň', 2);
show_words('empty', '', 1);

show_wrap('ordinary', 'The quick brown fox', 10, '|', false);
show_wrap('spaces', ' one  two   three ', 5, '<>', false);
show_wrap('cut', 'abcdefgh ij', 3, '|', true);
show_wrap('existing', 'ab<>cdef<>gh', 3, '<>', true);
show_wrap('zero', 'one two', 0, '|', false);
show_wrap('negative-cut', 'one two', -2, '|', true);
show_wrap('nul-break', "A\0B CDE F", 3, "\0|", true);
show_wrap('terminal-break', 'a |', 2, '|', false);
show_wrap('utf8-bytes', 'žluťoučký kůň', 4, '|', true);

$source = "A word\0tail";
$alias =& $source;
$wrapped = wordwrap($source, 3, "|\0", true);
$words = str_word_count($source, 2, "\0");
$source[0] = 'Z';
echo 'cow-source=', bin2hex($source), "\n";
echo 'cow-alias=', bin2hex($alias), "\n";
echo 'cow-wrapped=', bin2hex($wrapped), "\n";
echo 'cow-words=', render_words($words), "\n";
"#,
        ),
        r#"count=int:6
list=array:0:6f6e65,1:63616e2774,2:7461696c2d,3:612d62,4:7a,5:41
offsets=array:1:6f6e65,5:63616e2774,11:7461696c2d,17:612d62,24:7a,28:41
digits=array:1:6f6e65,5:63616e2774,11:7461696c2d,17:612d62,21:31325f7a,28:41
all-bytes=array:0:276f6e652063616e2774207461696c2d20612d622031325f7a2e00ff41
utf8-c-locale=array:2:6c75,6:6f75,10:6b,14:6b
empty=array:
ordinary=19:54686520717569636b7c62726f776e20666f78
spaces=21:206f6e65203c3e74776f20203c3e74687265653c3e
cut=13:6162637c6465667c67687c696a
existing=14:61623c3e6364653c3e663c3e6768
zero=7:6f6e657c74776f
negative-cut=13:7c6f7c6e7c657c7c747c777c6f
nul-break=11:410042204344007c452046
terminal-break=3:61207c
utf8-bytes=23:c5be6c757cc5a56f757cc48d6bc37cbd7c6bc5afc57c88
cow-source=5a20776f7264007461696c
cow-alias=5a20776f7264007461696c
cow-wrapped=417c00776f727c006400747c0061696c
cow-words=array:0:41,2:776f7264007461696c
"#,
    );
}

#[test]
fn word_boundary_and_wrap_own_weak_strict_and_diagnostic_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
function render(mixed $value): string {
    if (is_array($value)) return 'array:' . count($value) . ':' . bin2hex(implode(',', $value));
    return get_debug_type($value) . ':' . (is_string($value) ? bin2hex($value) : (string) $value);
}
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { echo render($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
final class TextValue {
    public function __construct(private string $value) {}
    public function __toString(): string { echo "convert\n"; return $this->value; }
}

attempt('count/int', static fn () => str_word_count(12.5));
attempt('count/null', static fn () => str_word_count(null));
attempt('count/object', static fn () => str_word_count(new TextValue('one two')));
attempt('count/array', static fn () => str_word_count([]));
attempt('count/format-string', static fn () => str_word_count('one two', '1'));
attempt('count/format-float', static fn () => str_word_count('one two', 1.5));
attempt('count/format-null', static fn () => str_word_count('one two', null));
attempt('count/chars-null', static fn () => str_word_count('12abc', 1, null));
attempt('count/chars-int', static fn () => str_word_count('12abc', 1, 12));
attempt('count/chars-invalid', static fn () => str_word_count('a.b', 2, '..'));
attempt('count/format-invalid', static fn () => str_word_count('abc', 3));
attempt('count/order', static fn () => str_word_count(new TextValue('abc'), new stdClass(), new TextValue('x')));

attempt('wrap/int', static fn () => wordwrap(12.5));
attempt('wrap/null', static fn () => wordwrap(null));
attempt('wrap/object', static fn () => wordwrap(new TextValue('one two'), 3, '|'));
attempt('wrap/array', static fn () => wordwrap([]));
attempt('wrap/width-string', static fn () => wordwrap('one two', '3', '|'));
attempt('wrap/width-float', static fn () => wordwrap('one two', 3.5, '|'));
attempt('wrap/width-null', static fn () => wordwrap('one two', null, '|'));
attempt('wrap/break-int', static fn () => wordwrap('one two', 3, 12));
attempt('wrap/break-null', static fn () => wordwrap('one two', 3, null));
attempt('wrap/cut-int', static fn () => wordwrap('abcdef', 3, '|', 1));
attempt('wrap/cut-null', static fn () => wordwrap('abcdef', 3, '|', null));
attempt('wrap/empty-break', static fn () => wordwrap('abc', 3, '', false));
attempt('wrap/zero-cut', static fn () => wordwrap('abc', 0, '|', true));
attempt('wrap/order', static fn () => wordwrap(new TextValue('abc'), new stdClass(), new TextValue('|'), new stdClass()));
restore_error_handler();
"#,
        ),
        r#"count/int=int:0
count/null=diag=8192:str_word_count(): Passing null to parameter #1 ($string) of type string is deprecated
int:0
count/object=convert
int:2
count/array=TypeError:str_word_count(): Argument #1 ($string) must be of type string, array given
count/format-string=array:2:6f6e652c74776f
count/format-float=diag=8192:Implicit conversion from float 1.5 to int loses precision
array:2:6f6e652c74776f
count/format-null=diag=8192:str_word_count(): Passing null to parameter #2 ($format) of type int is deprecated
int:2
count/chars-null=array:1:616263
count/chars-int=array:1:3132616263
count/chars-invalid=diag=2:str_word_count(): Invalid '..'-range, no character to the left of '..'
array:1:612e62
count/format-invalid=ValueError:str_word_count(): Argument #2 ($format) must be a valid format value
count/order=convert
TypeError:str_word_count(): Argument #2 ($format) must be of type int, stdClass given
wrap/int=string:31322e35
wrap/null=diag=8192:wordwrap(): Passing null to parameter #1 ($string) of type string is deprecated
string:
wrap/object=convert
string:6f6e657c74776f
wrap/array=TypeError:wordwrap(): Argument #1 ($string) must be of type string, array given
wrap/width-string=string:6f6e657c74776f
wrap/width-float=diag=8192:Implicit conversion from float 3.5 to int loses precision
string:6f6e657c74776f
wrap/width-null=diag=8192:wordwrap(): Passing null to parameter #2 ($width) of type int is deprecated
string:6f6e657c74776f
wrap/break-int=string:6f6e65313274776f
wrap/break-null=diag=8192:wordwrap(): Passing null to parameter #3 ($break) of type string is deprecated
ValueError:wordwrap(): Argument #3 ($break) must not be empty
wrap/cut-int=string:6162637c646566
wrap/cut-null=diag=8192:wordwrap(): Passing null to parameter #4 ($cut_long_words) of type bool is deprecated
string:616263646566
wrap/empty-break=ValueError:wordwrap(): Argument #3 ($break) must not be empty
wrap/zero-cut=ValueError:wordwrap(): Argument #4 ($cut_long_words) cannot be true when argument #2 ($width) is 0
wrap/order=convert
TypeError:wordwrap(): Argument #2 ($width) must be of type int, stdClass given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try {
        $value = $call();
        echo is_array($value) ? 'array:' . count($value) : get_debug_type($value) . ':' . (is_string($value) ? bin2hex($value) : (string) $value);
    } catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
attempt('count/valid', static fn () => str_word_count('one two', 1, null));
attempt('count/int', static fn () => str_word_count(1));
attempt('count/format', static fn () => str_word_count('abc', '1'));
attempt('count/chars', static fn () => str_word_count('abc', 1, 1));
attempt('wrap/valid', static fn () => wordwrap('one two', 3, '|', true));
attempt('wrap/int', static fn () => wordwrap(1));
attempt('wrap/width', static fn () => wordwrap('abc', '3'));
attempt('wrap/break', static fn () => wordwrap('abc', 3, 1));
attempt('wrap/cut', static fn () => wordwrap('abc', 3, '|', 1));
"#,
        ),
        r#"count/valid=array:2
count/int=TypeError:str_word_count(): Argument #1 ($string) must be of type string, int given
count/format=TypeError:str_word_count(): Argument #2 ($format) must be of type int, string given
count/chars=TypeError:str_word_count(): Argument #3 ($characters) must be of type ?string, int given
wrap/valid=string:6f6e657c74776f
wrap/int=TypeError:wordwrap(): Argument #1 ($string) must be of type string, int given
wrap/width=TypeError:wordwrap(): Argument #2 ($width) must be of type int, string given
wrap/break=TypeError:wordwrap(): Argument #3 ($break) must be of type string, int given
wrap/cut=TypeError:wordwrap(): Argument #4 ($cut_long_words) must be of type bool, int given
"#,
    );
}

#[test]
fn word_boundary_and_wrap_share_named_callback_reflection_and_order_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
function render_call(mixed $value): string {
    if (is_int($value)) return 'int:' . $value;
    if (is_string($value)) return 'string:' . bin2hex($value);
    $parts = [];
    foreach ($value as $key => $word) $parts[] = $key . ':' . bin2hex($word);
    return 'array:' . implode(',', $parts);
}
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { echo render_call($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
function mark(string $label, mixed $value): mixed { echo $label, '>'; return $value; }
final class OrderedText {
    public function __construct(private string $value) {}
    public function __toString(): string { echo 'convert>'; return $this->value; }
}

$dynamic = 'wordwrap';
$first = str_word_count(...);
attempt('named-count', static fn () => str_word_count(characters: '0..9', format: 2, string: 'a12 b'));
attempt('named-wrap', static fn () => wordwrap(cut_long_words: true, break: '|', width: 3, string: 'abcdef'));
attempt('dynamic', static fn () => ($GLOBALS['dynamic'])('one two', 3, '|'));
attempt('first-class', static fn () => ($GLOBALS['first'])('one two', 1));
attempt('call-user', static fn () => call_user_func('wordwrap', 'abcdef', 2, '|', true));
attempt('call-array', static fn () => call_user_func_array('str_word_count', ['format' => 2, 'string' => 'a b']));
attempt('too-many-count', static fn () => str_word_count('a', 0, null, 1));
attempt('too-many-wrap', static fn () => wordwrap('a', 1, '|', false, 1));
attempt('unknown', static fn () => wordwrap(string: 'abc', mystery: 1));
attempt('count-order', static fn () => str_word_count(
    mark('s', new OrderedText('abc')),
    mark('f', new stdClass()),
    mark('c', new OrderedText('x')),
));
attempt('wrap-order', static fn () => wordwrap(
    mark('s', 'x'), mark('w', 0), mark('b', ''), mark('c', new stdClass()),
));
attempt('semantic-order', static fn () => wordwrap('x', 0, '', true));
attempt('conversion-order', static fn () => wordwrap('x', 0, new OrderedText('|'), new stdClass()));

foreach (['str_word_count', 'wordwrap'] as $name) {
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
        r#"named-count=array:0:613132,4:62
named-wrap=string:6162637c646566
dynamic=string:6f6e657c74776f
first-class=array:0:6f6e65,1:74776f
call-user=string:61627c63647c6566
call-array=array:0:61,2:62
too-many-count=ArgumentCountError:str_word_count() expects at most 3 arguments, 4 given
too-many-wrap=ArgumentCountError:wordwrap() expects at most 4 arguments, 5 given
unknown=Error:Unknown named parameter $mystery
count-order=s>f>c>convert>TypeError:str_word_count(): Argument #2 ($format) must be of type int, stdClass given
wrap-order=s>w>b>c>TypeError:wordwrap(): Argument #4 ($cut_long_words) must be of type bool, stdClass given
semantic-order=ValueError:wordwrap(): Argument #3 ($break) must not be empty
conversion-order=convert>TypeError:wordwrap(): Argument #4 ($cut_long_words) must be of type bool, stdClass given
reflection=str_word_count:1/3:array|int
param=string:string:required:nonnull
param=format:int:optional:nonnull
param=characters:?string:optional:nullable
reflection=wordwrap:1/4:string
param=string:string:required:nonnull
param=width:int:optional:nonnull
param=break:string:optional:nonnull
param=cut_long_words:bool:optional:nonnull
"#,
    );
}
