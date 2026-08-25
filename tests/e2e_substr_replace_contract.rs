mod common;

use common::run_php;

#[test]
fn substr_replace_splices_php_bytes_and_broadcasts_array_values() {
    assert_eq!(
        run_php(
            r#"<?php
function show_substr_result(string $label, array|string $value): void {
    echo $label, '=';
    if (is_array($value)) {
        foreach ($value as $key => $item) {
            echo is_int($key) ? "i$key" : "s$key", ':', bin2hex($item), ';';
        }
    } else {
        echo bin2hex($value);
    }
    echo "\n";
}

$bytes = "\0A\xff\xc3\xa9Z";
show_substr_result('head', substr_replace($bytes, "\x80Q", -99));
show_substr_result('middle', substr_replace($bytes, "\x80Q", 1, -1));
show_substr_result('insert', substr_replace($bytes, "\x80Q", -1, PHP_INT_MIN));
show_substr_result('append', substr_replace($bytes, "\x80Q", PHP_INT_MAX, PHP_INT_MAX));

$subjects = [7 => 'abcdef', 'x' => 'uvwxyz', 2 => 'hi'];
show_substr_result('short-controls', substr_replace(
    $subjects,
    [9 => 'A', 3 => 'B', 20 => 'C', 21 => 'EXTRA'],
    [8 => 1, 4 => -2],
    [100 => 2],
));
show_substr_result('short-replace', substr_replace($subjects, ['X'], [1, 2, 3], [1, 1, 1]));
show_substr_result('empty-controls', substr_replace($subjects, [], [], []));

$replacement = ['L', 'unused', 'R'];
unset($replacement[1]);
$offset = [1, 99, -1];
unset($offset[1]);
$length = [2, 99, 0];
unset($length[1]);
show_substr_result('holes', substr_replace(['abcd', 'efgh', 'ijkl'], $replacement, $offset, $length));
show_substr_result('scalar-replace-array', substr_replace('abcdef', [8 => 'X', 9 => 'ignored'], 2, 2));

$source = ['a' => 'abcdef', 'b' => 'uvwxyz'];
$alias = &$source['a'];
$result = substr_replace($source, ['X', 'Y'], [1, 2], [3, 2]);
show_substr_result('source-before', $source);
show_substr_result('result-before', $result);
$result['a'] = 'changed';
show_substr_result('source-after', $source);
show_substr_result('result-after', $result);
echo 'alias=', bin2hex($alias), "\n";
"#,
        ),
        r#"head=8051
middle=0080515a
insert=0041ffc3a980515a
append=0041ffc3a95a8051
short-controls=i7:6141646566;sx:7576777842;i2:43;
short-replace=i7:615863646566;sx:757678797a;i2:6869;
empty-controls=i7:;sx:;i2:;
holes=i0:614c64;i1:6566675268;i2:;
scalar-replace-array=6162586566
source-before=sa:616263646566;sb:75767778797a;
result-before=sa:61586566;sb:757659797a;
source-after=sa:616263646566;sb:75767778797a;
result-after=sa:6368616e676564;sb:757659797a;
alias=616263646566
"#,
    );
}

#[test]
fn substr_replace_uses_php_weak_strict_and_nested_conversion_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
final class SliceText {
    public function __construct(private string $value) {}
    public function __toString(): string { return $this->value; }
}
function weak_slice(string $label, mixed $string, mixed $replace, mixed $offset, mixed $length): void {
    try {
        $result = substr_replace($string, $replace, $offset, $length);
        echo $label, '=';
        if (is_array($result)) {
            foreach ($result as $key => $item) echo $key, ':', bin2hex($item), ';';
        } else {
            echo bin2hex($result);
        }
        echo "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
weak_slice('source-null', null, 'X', 0, 0);
weak_slice('replace-bool', 'abc', true, 1, 1);
weak_slice('offset-float', 'abcdef', 'X', 2.9, 1);
weak_slice('length-float-string', 'abcdef', 'X', 1, '2.9');
weak_slice('source-stringable', new SliceText('TEXT'), 'X', 1, 2);
weak_slice('replace-stringable', 'abcd', new SliceText('XY'), 1, 2);
$resource = fopen('php://memory', 'r');
weak_slice('source-resource', $resource, 'X', 0, 0);
weak_slice('offset-object', 'abcd', 'X', new stdClass(), 1);
weak_slice(
    'items',
    [['a'], new SliceText('TEXT'), true],
    [['R'], new SliceText('V'), 9],
    [[1], '2.9', new stdClass()],
    [[1], '-1.9', false],
);
weak_slice('scalar-array-replace', 'abcd', [['X'], new SliceText('ignored')], 1, 2);
restore_error_handler();
"#,
        ),
        r#"diag=8192:substr_replace(): Passing null to parameter #1 ($string) of type array|string is deprecated
source-null=58
replace-bool=613163
diag=8192:Implicit conversion from float 2.9 to int loses precision
offset-float=616258646566
diag=8192:Implicit conversion from float-string "2.9" to int loses precision
length-float-string=6158646566
source-stringable=545854
replace-stringable=61585964
source-resource=TypeError:substr_replace(): Argument #1 ($string) must be of type array|string, resource given
offset-object=TypeError:substr_replace(): Argument #3 ($offset) must be of type array|int, stdClass given
diag=2:Array to string conversion
diag=2:Array to string conversion
diag=2:Object of class stdClass could not be converted to int
items=0:414172726179726179;1:54455654;2:3139;
diag=2:Array to string conversion
scalar-array-replace=61417272617964
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function strict_slice(string $label, mixed $string, mixed $replace, mixed $offset, mixed $length): void {
    try {
        $result = substr_replace($string, $replace, $offset, $length);
        echo $label, '=', is_array($result) ? count($result) : bin2hex($result), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
strict_slice('valid', 'abcdef', 'X', 1, null);
strict_slice('source-int', 42, 'X', 0, 0);
strict_slice('replace-bool', 'abc', true, 1, 1);
strict_slice('offset-string', 'abcdef', 'X', '1', 2);
strict_slice('length-float', 'abcdef', 'X', 1, 2.5);
strict_slice('array-items', [42, true], [false, 9], ['1.9', true], ['2.9', -1.9]);
"#,
        ),
        r#"valid=6158
source-int=TypeError:substr_replace(): Argument #1 ($string) must be of type array|string, int given
replace-bool=TypeError:substr_replace(): Argument #2 ($replace) must be of type array|string, true given
offset-string=TypeError:substr_replace(): Argument #3 ($offset) must be of type array|int, string given
length-float=TypeError:substr_replace(): Argument #4 ($length) must be of type array|int|null, float given
array-items=2
"#,
    );
}

#[test]
fn substr_replace_call_shapes_errors_and_reflection_share_one_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function show_call(string $label, callable $callback): void {
    try {
        $result = $callback();
        echo $label, '=';
        if (is_array($result)) {
            foreach ($result as $key => $item) echo $key, ':', bin2hex($item), ';';
        } else {
            echo bin2hex($result);
        }
        echo "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
show_call('static', static fn (): array|string => substr_replace('abcdef', 'X', 1, 2));
$dynamic = 'substr_replace';
show_call('dynamic', static fn (): array|string => $dynamic('abcdef', 'X', -2, 1));
show_call('callback', static fn (): mixed => call_user_func('substr_replace', 'abcdef', 'X', 2, 2));
show_call('callback-array', static fn (): mixed => call_user_func_array('substr_replace', ['abcdef', 'X', 3, 2]));
show_call('callback-named', static fn (): mixed => call_user_func_array('substr_replace', [
    'length' => 2,
    'string' => 'abcdef',
    'offset' => 1,
    'replace' => 'X',
]));
show_call('named', static fn (): array|string => substr_replace(offset: 1, replace: 'X', string: ['k' => 'abcdef'], length: 2));
show_call('offset-array-error', static fn (): array|string => substr_replace('abcdef', 'X', [1], 2));
show_call('length-array-error', static fn (): array|string => substr_replace('abcdef', 'X', 1, [2]));

$function = new ReflectionFunction('substr_replace');
echo 'arity=', $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), "\n";
foreach ($function->getParameters() as $parameter) {
    echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isOptional() ? 'optional' : 'required', ':',
        $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
}
echo 'return=', $function->getReturnType(), "\n";
"#,
        ),
        r#"static=6158646566
dynamic=616263645866
callback=6162586566
callback-array=6162635866
callback-named=6158646566
named=k:6158646566;
offset-array-error=TypeError:substr_replace(): Argument #3 ($offset) cannot be an array when working on a single string
length-array-error=TypeError:substr_replace(): Argument #4 ($length) cannot be an array when working on a single string
arity=3/4
param=string:array|string:required:nonnull
param=replace:array|string:required:nonnull
param=offset:array|int:required:nonnull
param=length:array|int|null:optional:nullable
return=array|string
"#,
    );
}
