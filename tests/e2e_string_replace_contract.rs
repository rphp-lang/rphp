mod common;

use common::run_php;

#[test]
fn replacement_is_ordered_byte_exact_and_preserves_array_references() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});

function show_replace(string $label, array|string $search, array|string $replace, array|string $subject, bool $insensitive = false): void {
    $count = 700;
    $result = $insensitive
        ? str_ireplace($search, $replace, $subject, $count)
        : str_replace($search, $replace, $subject, $count);
    echo $label, '=', is_array($result) ? serialize($result) : bin2hex($result), ':', $count, "\n";
}

show_replace('ordered', ['ab', 'X'], ['X', 'y'], 'zababa');
show_replace('short', ['a', 'b', 'c'], ['x'], 'abcabc');
show_replace('empty', ['', 'a', ''], ['q', 'x', 'z'], 'aba');
show_replace('keys', 'a', 'x', ['k' => 'aba', 4 => 'a']);
show_replace('nested', 'a', 'x', [['a'], 'a']);

$searchValue = 'a';
$replacementValue = 'x';
$subjectValue = 'aba';
$search = [&$searchValue, 'b'];
$replacement = [&$replacementValue, 'y'];
$subject = ['k' => &$subjectValue];
show_replace('refs', $search, $replacement, $subject);
echo 'state=', $searchValue, ':', $replacementValue, ':', $subjectValue, "\n";

show_replace('binary', "\x80\0", "\xff", "A\x80\0B\x80\0");
show_replace('binary-ci', ["a", "\x80\0"], ["Z", "\xff"], "Aa\x80\0B", true);
show_replace('utf8-ci', 'é', 'X', 'ÉéE', true);
"#,
        ),
        r#"ordered=7a797961:4
short=7878:6
empty=786278:2
keys=a:2:{s:1:"k";s:3:"xbx";i:4;s:1:"x";}:3
diag=2:Array to string conversion
nested=a:2:{i:0;s:5:"Arrxy";i:1;s:1:"x";}:2
refs=a:1:{s:1:"k";s:3:"xyx";}:3
state=a:x:aba
binary=41ff42ff:2
binary-ci=5a5aff42:3
utf8-ci=c3895845:1
"#,
    );
}

#[test]
fn replacement_uses_php_internal_weak_and_strict_union_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
class ReplaceText { public function __toString(): string { return 'a'; } }

function weak_case(string $label, mixed $search, mixed $replace, mixed $subject): void {
    $count = 91;
    try {
        $result = str_replace($search, $replace, $subject, $count);
        echo $label, '=', is_array($result) ? serialize($result) : bin2hex($result), ':', $count, "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), ':', $count, "\n";
    }
}

weak_case('null', null, 'x', 'aba');
weak_case('scalars', 1, 2.5, [true, 12.5, null]);
weak_case('stringable', new ReplaceText(), 'x', 'aba');
weak_case('plain-object', new stdClass(), 'x', 'aba');
weak_case('array-replace', 'a', ['x'], 'aba');
$resource = fopen('php://memory', 'r+');
weak_case('resource', $resource, 'x', 'aba');
fclose($resource);
"#,
        ),
        r#"diag=8192:str_replace(): Passing null to parameter #1 ($search) of type array|string is deprecated
null=616261:0
scalars=a:3:{i:0;s:3:"2.5";i:1;s:6:"2.52.5";i:2;s:0:"";}:2
stringable=786278:2
plain-object=TypeError:str_replace(): Argument #1 ($search) must be of type array|string, stdClass given:91
array-replace=TypeError:str_replace(): Argument #2 ($replace) must be of type string when argument #1 ($search) is a string:91
resource=TypeError:str_replace(): Argument #1 ($search) must be of type array|string, resource given:91
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function strict_case(string $label, mixed $search, mixed $replace, mixed $subject): void {
    $count = 91;
    try {
        $result = str_ireplace($search, $replace, $subject, $count);
        echo $label, '=', is_array($result) ? serialize($result) : bin2hex($result), ':', $count, "\n";
    } catch (Throwable $error) {
        echo $label, '=', $error::class, ':', $error->getMessage(), ':', $count, "\n";
    }
}
strict_case('valid', 'a', 'x', 'Aa');
strict_case('search-null', null, 'x', 'Aa');
strict_case('search-int', 1, 'x', 'Aa');
strict_case('replace-bool', 'a', true, 'Aa');
strict_case('subject-float', 'a', 'x', 1.5);
"#,
        ),
        r#"valid=7878:2
search-null=TypeError:str_ireplace(): Argument #1 ($search) must be of type array|string, null given:91
search-int=TypeError:str_ireplace(): Argument #1 ($search) must be of type array|string, int given:91
replace-bool=TypeError:str_ireplace(): Argument #2 ($replace) must be of type array|string, true given:91
subject-float=TypeError:str_ireplace(): Argument #3 ($subject) must be of type array|string, float given:91
"#,
    );
}

#[test]
fn replacement_count_signature_and_all_call_shapes_share_one_contract() {
    assert_eq!(
        run_php(
            r#"<?php
function show_call(string $label, callable $callback): void {
    $count = 700;
    $result = $callback($count);
    echo $label, '=', bin2hex($result), ':', $count, "\n";
}

show_call('static', fn(&$count) => str_ireplace('a', 'x', 'Aa', $count));
$dynamic = 'str_replace';
show_call('dynamic', fn(&$count) => $dynamic('a', 'x', 'aba', $count));
show_call('callback', fn(&$count) => call_user_func_array('str_ireplace', ['a', 'x', 'Aa', &$count]));
show_call('named', fn(&$count) => str_replace(subject: 'aba', replace: 'x', search: 'a', count: $count));

try {
    str_ireplace('a', 'x', 'a', 1);
} catch (Throwable $error) {
    echo 'nonref-count=', $error::class, ':', $error->getMessage(), "\n";
}

foreach (['str_replace', 'str_ireplace'] as $name) {
    $function = new ReflectionFunction($name);
    foreach ($function->getParameters() as $parameter) {
        echo $name, ':', $parameter->getName(), ':', $parameter->getType(), ':',
            $parameter->isPassedByReference() ? 'ref' : 'value', ':',
            $parameter->isOptional() ? 'optional' : 'required', "\n";
    }
    echo $name, ':return=', $function->getReturnType(), "\n";
}
"#,
        ),
        r#"static=7878:2
dynamic=786278:2
callback=7878:2
named=786278:2
nonref-count=Error:str_ireplace(): Argument #4 ($count) could not be passed by reference
str_replace:search:array|string:value:required
str_replace:replace:array|string:value:required
str_replace:subject:array|string:value:required
str_replace:count::ref:optional
str_replace:return=array|string
str_ireplace:search:array|string:value:required
str_ireplace:replace:array|string:value:required
str_ireplace:subject:array|string:value:required
str_ireplace:count::ref:optional
str_ireplace:return=array|string
"#,
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
try {
    call_user_func('str_replace', 1, 'x', '1');
} catch (Throwable $error) {
    echo 'call-user-func:', $error::class, ':', $error->getMessage(), "\n";
}
foreach (['direct', 'dynamic', 'callback'] as $shape) {
    $count = 9;
    try {
        if ($shape === 'direct') {
            str_replace(1, 'x', '1', $count);
        } elseif ($shape === 'dynamic') {
            $function = 'str_replace';
            $function(1, 'x', '1', $count);
        } else {
            call_user_func_array('str_replace', [1, 'x', '1', &$count]);
        }
    } catch (Throwable $error) {
        echo $shape, ':', $error::class, ':', $error->getMessage(), ':', $count, "\n";
    }
}
"#,
        ),
        r#"call-user-func:TypeError:str_replace(): Argument #1 ($search) must be of type array|string, int given
direct:TypeError:str_replace(): Argument #1 ($search) must be of type array|string, int given:9
dynamic:TypeError:str_replace(): Argument #1 ($search) must be of type array|string, int given:9
callback:TypeError:str_replace(): Argument #1 ($search) must be of type array|string, int given:9
"#,
    );
}
