mod common;

use common::run_php;

#[test]
fn nl2br_inserts_one_tag_before_each_php_newline_sequence() {
    assert_eq!(
        run_php(
            r#"<?php
function show_breaks(string $label, string $source, bool $xhtml = true): void {
    $before = bin2hex($source);
    $result = nl2br($source, $xhtml);
    echo $label, '=', bin2hex($result), '|',
        $before === bin2hex($source) ? 'same' : 'mutated', "\n";
}
show_breaks('empty', '');
show_breaks('plain', 'plain');
show_breaks('xhtml-pairs', "\r\r\n\n\r\n\r");
show_breaks('html-pairs', "\r\r\n\n\r\n\r", false);
show_breaks('bytes-xhtml', "A\0\nB\xff\rC\xc3\xa9\r\nD");
show_breaks('bytes-html', "A\0\nB\xff\rC\xc3\xa9\r\nD", false);
"#,
        ),
        r#"empty=|same
plain=706c61696e|same
xhtml-pairs=3c6272202f3e0d3c6272202f3e0d0a3c6272202f3e0a0d3c6272202f3e0a0d|same
html-pairs=3c62723e0d3c62723e0d0a3c62723e0a0d3c62723e0a0d|same
bytes-xhtml=41003c6272202f3e0a42ff3c6272202f3e0d43c3a93c6272202f3e0d0a44|same
bytes-html=41003c62723e0a42ff3c62723e0d43c3a93c62723e0d0a44|same
"#,
    );
}

#[test]
fn nl2br_uses_php_weak_and_strict_string_bool_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
final class BreakText {
    public function __toString(): string { return "A\nB"; }
}
function weak_break(string $label, mixed $string, mixed $xhtml): void {
    try {
        $result = nl2br($string, $xhtml);
        echo $label, '=', bin2hex($result), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
weak_break('null-string', null, true);
weak_break('false-string', false, true);
weak_break('true-string', true, true);
weak_break('int-string', 42, true);
weak_break('float-string', 2.5, true);
weak_break('stringable', new BreakText(), true);
weak_break('array-string', [], true);
$resource = fopen('php://memory', 'r');
weak_break('resource-string', $resource, true);
weak_break('null-bool', "A\nB", null);
weak_break('zero-bool', "A\nB", 0);
weak_break('string-zero-bool', "A\nB", '0');
weak_break('string-false-bool', "A\nB", 'false');
weak_break('object-bool', "A\nB", new stdClass());
weak_break('resource-bool', "A\nB", $resource);
restore_error_handler();
"#,
        ),
        r#"diag=8192:nl2br(): Passing null to parameter #1 ($string) of type string is deprecated
null-string=
false-string=
true-string=31
int-string=3432
float-string=322e35
stringable=413c6272202f3e0a42
array-string=TypeError:nl2br(): Argument #1 ($string) must be of type string, array given
resource-string=TypeError:nl2br(): Argument #1 ($string) must be of type string, resource given
diag=8192:nl2br(): Passing null to parameter #2 ($use_xhtml) of type bool is deprecated
null-bool=413c62723e0a42
zero-bool=413c62723e0a42
string-zero-bool=413c62723e0a42
string-false-bool=413c6272202f3e0a42
object-bool=TypeError:nl2br(): Argument #2 ($use_xhtml) must be of type bool, stdClass given
resource-bool=TypeError:nl2br(): Argument #2 ($use_xhtml) must be of type bool, resource given
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function strict_break(string $label, mixed $string, mixed $xhtml): void {
    try {
        $result = nl2br($string, $xhtml);
        echo $label, '=', bin2hex($result), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
strict_break('valid', "A\r\nB", false);
strict_break('string-int', 42, true);
strict_break('string-object', new stdClass(), true);
strict_break('bool-int', "A\nB", 1);
strict_break('bool-string', "A\nB", 'false');
strict_break('bool-null', "A\nB", null);
"#,
        ),
        r#"valid=413c62723e0d0a42
string-int=TypeError:nl2br(): Argument #1 ($string) must be of type string, int given
string-object=TypeError:nl2br(): Argument #1 ($string) must be of type string, stdClass given
bool-int=TypeError:nl2br(): Argument #2 ($use_xhtml) must be of type bool, int given
bool-string=TypeError:nl2br(): Argument #2 ($use_xhtml) must be of type bool, string given
bool-null=TypeError:nl2br(): Argument #2 ($use_xhtml) must be of type bool, null given
"#,
    );
}

#[test]
fn nl2br_call_shapes_and_reflection_share_one_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function show_call(string $label, callable $callback): void {
    try {
        $result = $callback();
        echo $label, '=', bin2hex($result), "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), "\n";
    }
}
show_call('static', static fn (): string => nl2br("A\nB"));
$dynamic = 'nl2br';
show_call('dynamic', static fn (): string => $dynamic("A\rB", false));
show_call('callback', static fn (): mixed => call_user_func('nl2br', "A\n\rB", true));
show_call('callback-named', static fn (): mixed => call_user_func_array('nl2br', [
    'use_xhtml' => false,
    'string' => "A\r\nB",
]));
show_call('named', static fn (): string => nl2br(use_xhtml: false, string: "A\nB"));
show_call('too-many', static fn (): string => nl2br("A\nB", true, false));

$function = new ReflectionFunction('nl2br');
echo 'arity=', $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), "\n";
foreach ($function->getParameters() as $parameter) {
    echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isOptional() ? 'optional' : 'required', ':',
        $parameter->allowsNull() ? 'nullable' : 'nonnull', "\n";
}
echo 'return=', $function->getReturnType(), "\n";
"#,
        ),
        r#"static=413c6272202f3e0a42
dynamic=413c62723e0d42
callback=413c6272202f3e0a0d42
callback-named=413c62723e0d0a42
named=413c62723e0a42
too-many=ArgumentCountError:nl2br() expects at most 2 arguments, 3 given
arity=1/2
param=string:string:required:nonnull
param=use_xhtml:bool:optional:nonnull
return=string
"#,
    );
}
