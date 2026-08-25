mod common;

use common::run_php;

#[test]
fn strtr_translates_php_bytes_with_both_non_recursive_schedules() {
    assert_eq!(
        run_php(
            r#"<?php
function bytes(string $label, string $value): void {
    echo $label, '=', bin2hex($value), "\n";
}
$binary = "A\0\x80\xffB\xc3\xa9";
bytes('character-binary', strtr($binary, "A\0\x80\xff\xc3", '12345'));
bytes('character-short-to', strtr('abca', 'abc', 'X'));
bytes('character-short-from', strtr('abca', 'a', 'XYZ'));
bytes('character-duplicate-last', strtr('aaaa', 'aa', 'XY'));
bytes('pairs-longest', strtr('abcababa', ['a' => '1', 'abc' => '3', 'ab' => '2']));
bytes('pairs-nonrecursive', strtr('aaaa', ['aa' => 'a', 'a' => 'z']));
bytes('pairs-empty-values', strtr('abcab', ['abc' => '', 'ab' => '', 'a' => 'X']));
bytes('pairs-binary-values', strtr("\x80Aé\0", ['A' => "\xff", 'é' => 'U', "\0" => 'N']));
bytes('pairs-integer-keys', strtr('x-1-01-12', [1 => 'I', '01' => 'Z', 12 => 'T']));

$source = 'foobar';
$sourceAlias =& $source;
$replacement = 'R';
$replacementAlias =& $replacement;
$pairs = ['foo' => &$replacementAlias, 'bar' => 'Y'];
bytes('references', strtr($source, $pairs));
bytes('source-after', $source);
bytes('alias-after', $sourceAlias);
bytes('replacement-after', $replacement);
"#,
        ),
        r#"character-binary=313233344235a9
character-short-to=58626358
character-short-from=58626358
character-duplicate-last=59595959
pairs-longest=33323231
pairs-nonrecursive=6161
pairs-empty-values=
pairs-binary-values=80ff554e
pairs-integer-keys=782d492d5a2d54
references=5259
source-after=666f6f626172
alias-after=666f6f626172
replacement-after=52
"#,
    );
}

#[test]
fn strtr_uses_php_weak_strict_and_lazy_replacement_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
final class Text {
    public function __construct(private string $text) {}
    public function __toString(): string { echo "convert={$this->text}\n"; return $this->text; }
}
final class Plain {}
function attempt(string $label, callable $call): void {
    echo "[$label]\n";
    try { $value = $call(); echo bin2hex($value), "\n"; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
attempt('null-subject', static fn () => strtr(null, []));
attempt('scalar-subject', static fn () => strtr(120.5, ['2' => 'X']));
attempt('stringable-subject', static fn () => strtr(new Text('ab'), ['a' => 'X']));
attempt('array-subject', static fn () => strtr([], []));
attempt('null-from', static fn () => strtr('abc', null, 'X'));
attempt('bool-from', static fn () => strtr('101', true, 'X'));
attempt('array-from-three', static fn () => strtr('abc', ['a' => 'X'], 'Y'));
attempt('null-to', static fn () => strtr('abc', 'a', null));
attempt('object-to', static fn () => strtr('abc', 'a', new Plain()));
attempt('scalar-pairs', static fn () => strtr('abc', true));

$once = new Text('Q');
attempt('cached-stringable', static fn () => strtr('aaaa', ['a' => $once]));
attempt('unused-stringable', static fn () => strtr('a', ['a' => 'X', 'zz' => new Text('U')]));
attempt('array-value', static fn () => strtr('aa', ['a' => []]));
attempt('bad-object-value', static fn () => strtr('a', ['a' => new Plain()]));
attempt('empty-subject-is-lazy', static fn () => strtr('', ['' => new Text('E'), 'a' => new Text('A')]));
attempt('empty-key', static fn () => strtr('a', ['' => new Text('E'), 'a' => new Text('A')]));
restore_error_handler();
"#,
        ),
        r#"[null-subject]
diag=8192:strtr(): Passing null to parameter #1 ($string) of type string is deprecated

[scalar-subject]
3158302e35
[stringable-subject]
convert=ab
5862
[array-subject]
TypeError:strtr(): Argument #1 ($string) must be of type string, array given
[null-from]
diag=8192:strtr(): Passing null to parameter #2 ($from) of type array|string is deprecated
616263
[bool-from]
583058
[array-from-three]
TypeError:strtr(): Argument #2 ($from) must be of type string, array given
[null-to]
diag=8192:strtr(): Passing null to parameter #3 ($to) of type ?string is deprecated
616263
[object-to]
TypeError:strtr(): Argument #3 ($to) must be of type string, Plain given
[scalar-pairs]
TypeError:strtr(): Argument #2 ($from) must be of type array, true given
[cached-stringable]
convert=Q
51515151
[unused-stringable]
58
[array-value]
diag=2:Array to string conversion
41727261794172726179
[bad-object-value]
Error:Object of class Plain could not be converted to string
[empty-subject-is-lazy]

[empty-key]
diag=2:strtr(): Ignoring replacement of empty string
convert=A
41
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
final class Text { public function __toString(): string { return 'a'; } }
function attempt(string $label, callable $call): void {
    echo "[$label]\n";
    try { var_dump($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
attempt('valid-two', static fn () => strtr('abc', ['a' => 'X']));
attempt('valid-three', static fn () => strtr('abc', 'a', 'X'));
attempt('subject-int', static fn () => strtr(1, []));
attempt('subject-stringable', static fn () => strtr(new Text(), []));
attempt('from-null', static fn () => strtr('abc', null, 'X'));
attempt('from-int', static fn () => strtr('abc', 1, 'X'));
attempt('to-null', static fn () => strtr('abc', 'a', null));
attempt('pairs-int', static fn () => strtr('abc', 1));
"#,
        ),
        r#"[valid-two]
string(3) "Xbc"
[valid-three]
string(3) "Xbc"
[subject-int]
TypeError:strtr(): Argument #1 ($string) must be of type string, int given
[subject-stringable]
TypeError:strtr(): Argument #1 ($string) must be of type string, Text given
[from-null]
TypeError:strtr(): Argument #2 ($from) must be of type string, null given
[from-int]
TypeError:strtr(): Argument #2 ($from) must be of type string, int given
[to-null]
TypeError:strtr(): Argument #3 ($to) must be of type string, null given
[pairs-int]
TypeError:strtr(): Argument #2 ($from) must be of type array, int given
"#,
    );
}

#[test]
fn strtr_call_shapes_reflection_and_heredoc_share_the_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function attempt(string $label, callable $call): void {
    echo "[$label]\n";
    try { var_dump($call()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
$dynamic = 'strtr';
attempt('static-three', static fn () => strtr('abc', 'ab', 'XY'));
attempt('dynamic-two', static fn () => $dynamic('abc', ['ab' => 'Z']));
attempt('named-three', static fn () => strtr(to: 'XY', string: 'abc', from: 'ab'));
attempt('named-two', static fn () => strtr(from: ['ab' => 'Z'], string: 'abc'));
attempt('callback', static fn () => call_user_func('strtr', 'abc', ['ab' => 'Z']));
attempt('callback-named', static fn () => call_user_func_array('strtr', [
    'from' => ['ab' => 'Z'], 'string' => 'abc',
]));
attempt('too-few', static fn () => strtr('abc'));
attempt('too-many', static fn () => strtr('abc', 'a', 'X', 'ignored'));
attempt('wrong-two-arg-overload', static fn () => strtr('abc', 'a'));
attempt('wrong-three-arg-overload', static fn () => strtr('abc', ['a' => 'X'], 'Y'));
attempt('unknown-named', static fn () => strtr(string: 'abc', from: [], extra: true));

$value = 'V';
$document = <<<DOC
\"|$value
DOC;
echo 'heredoc=', bin2hex($document), "\n";

$function = new ReflectionFunction('strtr');
echo 'arity=', $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), "\n";
foreach ($function->getParameters() as $index => $parameter) {
    echo 'param=', $index, ':', $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isOptional() ? 'optional' : 'required', ':',
        $parameter->allowsNull() ? 'nullable' : 'nonnull', ':',
        $parameter->isPassedByReference() ? 'ref' : 'value', "\n";
}
echo 'return=', $function->getReturnType(), "\n";
"#,
        ),
        r#"[static-three]
string(3) "XYc"
[dynamic-two]
string(2) "Zc"
[named-three]
string(3) "XYc"
[named-two]
string(2) "Zc"
[callback]
string(2) "Zc"
[callback-named]
string(2) "Zc"
[too-few]
ArgumentCountError:strtr() expects exactly 2 arguments, 1 given
[too-many]
ArgumentCountError:strtr() expects exactly 3 arguments, 4 given
[wrong-two-arg-overload]
TypeError:strtr(): Argument #2 ($from) must be of type array, string given
[wrong-three-arg-overload]
TypeError:strtr(): Argument #2 ($from) must be of type string, array given
[unknown-named]
Error:Unknown named parameter $extra
heredoc=5c227c56
arity=2/3
param=0:string:string:required:nonnull:value
param=1:from:array|string:required:nonnull:value
param=2:to:?string:optional:nullable:value
return=string
"#,
    );
}
