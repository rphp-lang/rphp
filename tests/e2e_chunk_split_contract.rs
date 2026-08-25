mod common;

use common::run_php;

#[test]
fn chunk_split_is_byte_exact_and_appends_every_separator() {
    assert_eq!(
        run_php(
            r#"<?php
function show_chunk(string $label, string $source, int $length, string $separator): void {
    echo $label, '=', bin2hex(chunk_split($source, $length, $separator)), "\n";
}

show_chunk('empty', '', 7, '|');
show_chunk('partial', 'abcde', 2, '<>');
show_chunk('wide', 'abc', PHP_INT_MAX, '|');
show_chunk('empty-separator', 'abcde', 2, '');
show_chunk('nul', "a\0bc", 2, "\xff");
show_chunk('utf8-byte', "éZ", 1, '|');
show_chunk('utf8-pair', "éZ", 2, '|');
show_chunk('invalid', "\x80A\xff", 2, "é");
echo 'default=', bin2hex(chunk_split('abc')), "\n";

$source = "\x80Z";
$copy = $source;
chunk_split($source, 1, '|');
echo 'immutable=', bin2hex($source), ':', bin2hex($copy), "\n";

$dynamic = 'chunk_split';
echo 'dynamic=', bin2hex($dynamic('abc', 2, '|')), "\n";
echo 'callback=', bin2hex(call_user_func('chunk_split', 'abc', 2, '|')), "\n";
echo 'named=', bin2hex(chunk_split(separator: '|', string: 'abc', length: 2)), "\n";

$function = new ReflectionFunction('chunk_split');
foreach ($function->getParameters() as $parameter) {
    echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':', $parameter->isOptional() ? 'optional' : 'required', "\n";
}
echo 'return=', $function->getReturnType(), "\n";
"#,
        ),
        r#"empty=7c
partial=61623c3e63643c3e653c3e
wide=6162637c
empty-separator=6162636465
nul=6100ff6263ff
utf8-byte=c37ca97c5a7c
utf8-pair=c3a97c5a7c
invalid=8041c3a9ffc3a9
default=6162630d0a
immutable=805a:805a
dynamic=61627c637c
callback=61627c637c
named=61627c637c
param=string:string:required
param=length:int:optional
param=separator:string:optional
return=string
"#,
    );
}

#[test]
fn chunk_split_uses_php_weak_scalar_coercion_and_catchable_errors() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
class ChunkText { public function __toString(): string { return 'xy'; } }

function weak_chunk(string $label, mixed $source, mixed $length, mixed $separator): void {
    try {
        $result = chunk_split($source, $length, $separator);
        echo $label, '=', bin2hex($result), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
function weak_unary_chunk(string $label, mixed $source): void {
    try {
        $result = chunk_split($source);
        echo $label, '=', bin2hex($result), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
        if ($label === 'unary-array') {
            echo 'unary-trace=', $error->getTrace()[0]['function'], "\n";
        }
    }
}

weak_unary_chunk('unary-string', 'abc');
weak_unary_chunk('unary-null', null);
weak_unary_chunk('unary-object', new ChunkText());
weak_unary_chunk('unary-array', []);
weak_chunk('source-null', null, 1, '|');
weak_chunk('source-bool', true, 1, '|');
weak_chunk('source-number', 12.5, 2, '|');
weak_chunk('source-object', new ChunkText(), 1, '|');
weak_chunk('source-array', [], 1, '|');
weak_chunk('length-null', 'abcd', null, '|');
weak_chunk('length-float', 'abcd', 2.5, '|');
weak_chunk('length-string', 'abcd', '2', '|');
weak_chunk('length-nan', 'abcd', NAN, '|');
weak_chunk('length-zero', 'abcd', 0, '|');
weak_chunk('length-negative', 'abcd', -2, '|');
weak_chunk('separator-null', 'abcd', 2, null);
weak_chunk('separator-bool', 'abcd', 2, true);
weak_chunk('separator-number', 'abcd', 2, 12.5);
weak_chunk('separator-object', 'abcd', 2, new ChunkText());
weak_chunk('separator-array', 'abcd', 2, []);
"#,
        ),
        r#"unary-string=6162630d0a
diag=8192:chunk_split(): Passing null to parameter #1 ($string) of type string is deprecated
unary-null=0d0a
unary-object=78790d0a
unary-array=TypeError:chunk_split(): Argument #1 ($string) must be of type string, array given
unary-trace=chunk_split
diag=8192:chunk_split(): Passing null to parameter #1 ($string) of type string is deprecated
source-null=7c
source-bool=317c
source-number=31327c2e357c
source-object=787c797c
source-array=TypeError:chunk_split(): Argument #1 ($string) must be of type string, array given
diag=8192:chunk_split(): Passing null to parameter #2 ($length) of type int is deprecated
length-null=ValueError:chunk_split(): Argument #2 ($length) must be greater than 0
diag=8192:Implicit conversion from float 2.5 to int loses precision
length-float=61627c63647c
length-string=61627c63647c
length-nan=TypeError:chunk_split(): Argument #2 ($length) must be of type int, float given
length-zero=ValueError:chunk_split(): Argument #2 ($length) must be greater than 0
length-negative=ValueError:chunk_split(): Argument #2 ($length) must be greater than 0
diag=8192:chunk_split(): Passing null to parameter #3 ($separator) of type string is deprecated
separator-null=61626364
separator-bool=616231636431
separator-number=616231322e35636431322e35
separator-object=6162787963647879
separator-array=TypeError:chunk_split(): Argument #3 ($separator) must be of type string, array given
"#,
    );
}

#[test]
fn chunk_split_rejects_weak_only_values_in_strict_calls() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function strict_chunk(string $label, mixed $source, mixed $length, mixed $separator): void {
    try {
        $result = chunk_split($source, $length, $separator);
        echo $label, '=', bin2hex($result), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}

strict_chunk('valid', 'abcd', 2, '|');
strict_chunk('source-null', null, 1, '|');
strict_chunk('source-int', 1, 1, '|');
strict_chunk('source-object', new stdClass(), 1, '|');
strict_chunk('length-null', 'abcd', null, '|');
strict_chunk('length-bool', 'abcd', true, '|');
strict_chunk('length-string', 'abcd', '2', '|');
strict_chunk('separator-null', 'abcd', 2, null);
strict_chunk('separator-int', 'abcd', 2, 1);
strict_chunk('separator-array', 'abcd', 2, []);
"#,
        ),
        r#"valid=61627c63647c
source-null=TypeError:chunk_split(): Argument #1 ($string) must be of type string, null given
source-int=TypeError:chunk_split(): Argument #1 ($string) must be of type string, int given
source-object=TypeError:chunk_split(): Argument #1 ($string) must be of type string, stdClass given
length-null=TypeError:chunk_split(): Argument #2 ($length) must be of type int, null given
length-bool=TypeError:chunk_split(): Argument #2 ($length) must be of type int, true given
length-string=TypeError:chunk_split(): Argument #2 ($length) must be of type int, string given
separator-null=TypeError:chunk_split(): Argument #3 ($separator) must be of type string, null given
separator-int=TypeError:chunk_split(): Argument #3 ($separator) must be of type string, int given
separator-array=TypeError:chunk_split(): Argument #3 ($separator) must be of type string, array given
"#,
    );
}
