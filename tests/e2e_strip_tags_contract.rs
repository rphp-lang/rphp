mod common;

use common::run_php;

#[test]
fn strip_tags_accepts_spaced_document_openers_and_scans_general_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
$one = <<<   DOC
<!DOCTYPE q '>'><b>A</b><!-- I've gone -->
DOC;
$two = <<< "DOC"
<?= '<?= 1 ?>' ?>B<?xml:n p='>' />
DOC;
$three = <<<   'DOC'
<<HtMl>>C<</HtMl>><a.>D</.a>< ax
DOC;
foreach ([
    [$one, null],
    [$one, '<b>'],
    [$two, null],
    [$three, '<<html>>'],
] as $case) {
    $result = strip_tags($case[0], $case[1]);
    echo strlen($result), ':', bin2hex($result), "\n";
}
"#,
        ),
        concat!(
            "1:41\n",
            "8:3c623e413c2f623e\n",
            "1:42\n",
            "19:3c48744d6c3e433c2f48744d6c3e443c206178\n",
        )
    );
}

#[test]
fn strip_tags_matches_generated_php_85_state_and_byte_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
$records = [];
$record = static function (string $source, array|string|null $allowed = null) use (&$records): void {
    $result = strip_tags($source, $allowed);
    $records[] = strlen($result) . ':' . md5($result);
};

$syntax = [
    'A<B', 'A<B>C', 'A<>B', 'A<<b>>C<</b>>D', 'A< b>C', 'A</ b>C',
    'A<a.>B</.a>C', 'A<1x>B</1x>C', 'A<a/b>B', 'A<a/>B<a / >C',
    'A<! x>B', 'A<!x>B', "A<!DOCTYPE q '>'>B", "A<!-- I've > x -->B",
    'A<!-->B<!-- x --!>C<!-- y --->D', "A<? echo '?>'; ?>B",
    "A<?= '<?= 1 ?>' ?>B", "A<?xml:n p='?>' ?>B", "A<% echo '%>'; %>B",
    "A<a x='>' y=\"<\">B</a>C", 'A<a <b>>C</a>D', "A<a x='>' B",
    'A<!-- x B', 'A<? x B',
];
foreach ($syntax as $source) {
    $record($source);
}

$allowedSources = [
    "A<A X='1'>B</A><b>C</b><a/b>D<a/>E",
    'A<<HtMl>>B<</HtMl>>C',
    'A<a.>B</.a><a-b>C</a-b><a:b>D</a:b>',
];
$allowedLists = [null, '', '<a>', 'a', 'junk<A x><B></b>', '<<html>>', ['A', '<b>', 1, null, false]];
foreach ($allowedSources as $source) {
    foreach ($allowedLists as $allowed) {
        $record($source, $allowed);
    }
}

foreach ([
    "A\0<a>B\0</a>C\0",
    "A<a\0x>B</a\0x>C",
    "A<a x='q\0r'>B</a>C",
    "A\xff<a>B</a>\xfeC",
    "A<a\xff>B</a\xff>C",
] as $source) {
    $record($source, '<a>');
}

$prefixes = ['<', '</', '<!', '<!--', '<?', '<?=', '<%', '<a', '<a/', '<a ', "<a x='"];
$suffixes = ['', '>', 'x', 'x>', "'", "'>", '?>', '-->'];
foreach ($prefixes as $prefix) {
    foreach ($suffixes as $suffix) {
        $record('L' . $prefix . $suffix . 'R');
    }
}
echo count($records), ':', md5(implode("\n", $records)), "\n";
"#,
        ),
        "138:d6338ab0c84ce7f9568e5527753cf9d7\n"
    );
}

#[test]
fn strip_tags_preserves_call_order_cow_binary_metadata_and_call_forms() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});

class SourceText {
    public function __toString(): string {
        echo "source-cast\n";
        return "\0<b>B</b><i>I</i>\xff";
    }
}
class TagName {
    public function __toString(): string {
        echo "allowed-cast\n";
        $GLOBALS['allowed'][] = 'i';
        return 'b';
    }
}
class AllowedText {
    public function __toString(): string {
        echo "top-allowed-cast\n";
        return '<i>';
    }
}

$allowed = [new TagName(), 1, null, false];
$alias =& $allowed;
$result = strip_tags(new SourceText(), $allowed);
echo 'object=', strlen($result), ':', bin2hex($result), ':', count($allowed), ':', count($alias), "\n";

$source = '<b>X</b><i>Y</i>';
$copy = $source;
echo 'cow=', bin2hex(strip_tags($source, ['b'])), ':', bin2hex($source), ':', bin2hex($copy), "\n";
echo 'top-allowed=', bin2hex(strip_tags($source, new AllowedText())), "\n";

$name = 'strip_tags';
$first = strip_tags(...);
$calls = [
    'named' => static fn () => strip_tags(allowed_tags: '<i>', string: '<b>X</b><i>Y</i>'),
    'dynamic' => static fn () => ($GLOBALS['name'])('<b>X</b><i>Y</i>', '<b>'),
    'first' => static fn () => ($GLOBALS['first'])('<b>X</b><i>Y</i>', '<i>'),
    'call-user' => static fn () => call_user_func('strip_tags', '<b>X</b><i>Y</i>', '<b>'),
    'call-array' => static fn () => call_user_func_array('strip_tags', ['allowed_tags' => '<i>', 'string' => '<b>X</b><i>Y</i>']),
];
foreach ($calls as $label => $call) {
    echo $label, '=', bin2hex($call()), "\n";
}

echo 'null=', bin2hex(strip_tags(null)), "\n";
try {
    strip_tags('<b>X</b>', [[], new stdClass()]);
} catch (Throwable $error) {
    echo 'array-error=', $error::class, ':', $error->getMessage(), "\n";
}

$reflection = new ReflectionFunction('strip_tags');
echo 'reflection=', $reflection->getName(), ':', $reflection->getNumberOfRequiredParameters(), '/', $reflection->getNumberOfParameters(), ':', $reflection->getReturnType(), "\n";
foreach ($reflection->getParameters() as $parameter) {
    echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':', $parameter->allowsNull() ? 'null' : 'nonnull', ':', $parameter->isOptional() ? 'optional' : 'required', ':';
    echo $parameter->isDefaultValueAvailable() ? var_export($parameter->getDefaultValue(), true) : '-', "\n";
}
$reflectionText = (string) $reflection;
echo 'reflection-text=', strlen($reflectionText), ':', md5($reflectionText), "\n";
restore_error_handler();
"#,
        ),
        concat!(
            "source-cast\n",
            "allowed-cast\n",
            "object=10:3c623e423c2f623e49ff:5:5\n",
            "cow=3c623e583c2f623e59:3c623e583c2f623e3c693e593c2f693e:3c623e583c2f623e3c693e593c2f693e\n",
            "top-allowed=top-allowed-cast\n",
            "583c693e593c2f693e\n",
            "named=583c693e593c2f693e\n",
            "dynamic=3c623e583c2f623e59\n",
            "first=583c693e593c2f693e\n",
            "call-user=3c623e583c2f623e59\n",
            "call-array=583c693e593c2f693e\n",
            "null=diag=8192:strip_tags(): Passing null to parameter #1 ($string) of type string is deprecated\n",
            "\n",
            "diag=2:Array to string conversion\n",
            "array-error=Error:Object of class stdClass could not be converted to string\n",
            "reflection=strip_tags:1/2:string\n",
            "param=string:string:nonnull:required:-\n",
            "param=allowed_tags:array|string|null:null:optional:NULL\n",
            "reflection-text=223:bba42ba2c6ccba33585b86af23bf2633\n",
        )
    );
}

#[test]
fn strip_tags_enforces_strict_php_85_argument_types() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class Text {
    public function __toString(): string {
        echo "cast\n";
        return '<b>X</b>';
    }
}
function attempt(string $label, Closure $call): void {
    echo $label, '=';
    try {
        echo bin2hex($call());
    } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage();
    }
    echo "\n";
}
attempt('first-int', static fn () => strip_tags(1));
attempt('first-array', static fn () => strip_tags([]));
attempt('first-object', static fn () => strip_tags(new Text()));
attempt('second-bool', static fn () => strip_tags('<b>X</b>', false));
attempt('second-int', static fn () => strip_tags('<b>X</b>', 1));
attempt('second-object', static fn () => strip_tags('<b>X</b>', new Text()));
attempt('second-null', static fn () => strip_tags('<b>X</b>', null));
attempt('second-array', static fn () => strip_tags('<b>X</b>', ['b']));
"#,
        ),
        concat!(
            "first-int=TypeError:strip_tags(): Argument #1 ($string) must be of type string, int given\n",
            "first-array=TypeError:strip_tags(): Argument #1 ($string) must be of type string, array given\n",
            "first-object=TypeError:strip_tags(): Argument #1 ($string) must be of type string, Text given\n",
            "second-bool=TypeError:strip_tags(): Argument #2 ($allowed_tags) must be of type array|string|null, false given\n",
            "second-int=TypeError:strip_tags(): Argument #2 ($allowed_tags) must be of type array|string|null, int given\n",
            "second-object=TypeError:strip_tags(): Argument #2 ($allowed_tags) must be of type array|string|null, Text given\n",
            "second-null=58\n",
            "second-array=3c623e583c2f623e\n",
        )
    );
}
