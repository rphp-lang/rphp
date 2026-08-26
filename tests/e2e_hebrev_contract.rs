mod common;

use common::run_php;

#[test]
fn hebrev_matches_legacy_byte_runs_punctuation_lines_and_wrapping() {
    assert_eq!(
        run_php(
            r#"<?php
function show(string $label, string $source, int $width = 0): void {
    $before = bin2hex($source);
    $result = hebrev($source, $width);
    echo $label, '=', $width, ':', bin2hex($result), ':',
        $before === bin2hex($source) ? 'same' : 'mutated', "\n";
}
show('empty', '');
show('ascii', 'abc def');
show('punct', 'abc def.');
show('hebrew', "AB\xe0\xe1CD");
show('neutral-left', "A.\xe0\xe1");
show('neutral-right', "\xe0\xe1!B");
show('weak-left', "A-/\xe0");
show('weak-right', "\xe0-/A");
show('mirrors', "A(\xe0[\xe1{B");
show('bytes', "\0A\xffB");
show('utf8', "שלום (abc)");
show('lines', "abc.\nAB\xe0\xe1CD\nlast!");
show('crlf', "left\r\nright");
show('wrap', "The hebrev function converts logical Hebrew text to visual text.", 15);
show('word-exact', 'a b', 3);
show('word-break', 'a b', 2);
show('long-word', 'abcdefgh', 3);
show('wide', 'abc def.', 99);
show('negative', 'abc.', -1);
show('negative-space', 'a b', -5);
show('negative-mixed', "\xe0\xe1A", -2);
"#,
        ),
        concat!(
            "empty=0::same\n",
            "ascii=0:61626320646566:same\n",
            "punct=0:2e61626320646566:same\n",
            "hebrew=0:4344e1e04142:same\n",
            "neutral-left=0:e1e02e41:same\n",
            "neutral-right=0:4221e1e0:same\n",
            "weak-left=0:e0412d2f:same\n",
            "weak-right=0:415c2de0:same\n",
            "mirrors=0:427de15de02941:same\n",
            "bytes=0:0041ff42:same\n",
            "utf8=0:28d7a9d79cd795d79d2028616263:same\n",
            "lines=0:2e6162630a4344e1e041420a216c617374:same\n",
            "crlf=0:6c6566740d0a7269676874:same\n",
            "wrap=15:746f2076697375616c20746578740a48656272657720746578740a6c6f676963616c0a636f6e76657274730a6865627265762066756e6374696f6e0a2e546865:same\n",
            "word-exact=3:612062:same\n",
            "word-break=2:620a61:same\n",
            "long-word=3:6566676861626364:same\n",
            "wide=99:2e61626320646566:same\n",
            "negative=-1:6362612e:same\n",
            "negative-space=-5:620a61:same\n",
            "negative-mixed=-2:e0e141:same\n",
        )
    );
}

#[test]
fn hebrev_preserves_call_shapes_reflection_references_and_cow() {
    assert_eq!(
        run_php(
            r#"<?php
$source = "AB\xe0\xe1CD.";
$copy = $source;
$reference =& $source;
$dynamic = 'hebrev';
$firstClass = hebrev(...);
$calls = [
    hebrev($source),
    $dynamic($source),
    $firstClass($source),
    call_user_func('hebrev', $source),
    hebrev(string: $source, max_chars_per_line: 0),
    hebrev(...[$source, 0]),
];
foreach ($calls as $value) {
    echo bin2hex($value), "\n";
}
echo $source === $copy && $source === $reference ? "stable\n" : "mutated\n";

$reflection = new ReflectionFunction('hebrev');
echo $reflection->getName(), '|', $reflection->getNumberOfRequiredParameters(), '/',
    $reflection->getNumberOfParameters(), '|', $reflection->getReturnType(), "\n";
foreach ($reflection->getParameters() as $parameter) {
    echo $parameter->getName(), ':', $parameter->getType(), ':',
        $parameter->isDefaultValueAvailable()
            ? var_export($parameter->getDefaultValue(), true)
            : '-', "\n";
}
"#,
        ),
        concat!(
            "2e4344e1e04142\n",
            "2e4344e1e04142\n",
            "2e4344e1e04142\n",
            "2e4344e1e04142\n",
            "2e4344e1e04142\n",
            "2e4344e1e04142\n",
            "stable\n",
            "hebrev|1/2|string\n",
            "string:string:-\n",
            "max_chars_per_line:int:0\n",
        )
    );
}

#[test]
fn hebrev_matches_weak_strict_and_side_effect_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
class HebrewString {
    public function __toString(): string { echo 'cast|'; return 'abc.'; }
}
class ThrowingHebrewString {
    public function __toString(): string { echo 'throw|'; throw new Exception('stop'); }
}
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message|";
    return true;
});
function attempt(callable $callback): void {
    try { echo bin2hex($callback()), "\n"; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
attempt(static fn () => hebrev(null, 0));
attempt(static fn () => hebrev(false, 0));
attempt(static fn () => hebrev(true, 0));
attempt(static fn () => hebrev(42, 0));
attempt(static fn () => hebrev(2.5, 0));
attempt(static fn () => hebrev(new HebrewString, 0));
attempt(static fn () => hebrev([], 0));
attempt(static fn () => hebrev('abc', null));
attempt(static fn () => hebrev('abc', true));
attempt(static fn () => hebrev('abc', 2.5));
attempt(static fn () => hebrev('abc', '2'));
attempt(static fn () => hebrev('abc', []));
attempt(static fn () => hebrev(new ThrowingHebrewString, 0));
restore_error_handler();
"#,
        ),
        concat!(
            "diag=8192:hebrev(): Passing null to parameter #1 ($string) of type string is deprecated|\n",
            "\n",
            "31\n",
            "3432\n",
            "322e35\n",
            "cast|2e616263\n",
            "TypeError:hebrev(): Argument #1 ($string) must be of type string, array given\n",
            "diag=8192:hebrev(): Passing null to parameter #2 ($max_chars_per_line) of type int is deprecated|616263\n",
            "626361\n",
            "diag=8192:Implicit conversion from float 2.5 to int loses precision|616263\n",
            "616263\n",
            "TypeError:hebrev(): Argument #2 ($max_chars_per_line) must be of type int, array given\n",
            "throw|Exception:stop\n",
        )
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class StrictHebrewString { public function __toString(): string { return 'abc'; } }
function attempt(callable $callback): void {
    try { echo bin2hex($callback()), "\n"; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
attempt(static fn () => hebrev(1, 0));
attempt(static fn () => hebrev(new StrictHebrewString, 0));
attempt(static fn () => hebrev('abc', true));
attempt(static fn () => hebrev('abc', '2'));
attempt(static fn () => hebrev('abc', null));
"#,
        ),
        concat!(
            "TypeError:hebrev(): Argument #1 ($string) must be of type string, int given\n",
            "TypeError:hebrev(): Argument #1 ($string) must be of type string, StrictHebrewString given\n",
            "TypeError:hebrev(): Argument #2 ($max_chars_per_line) must be of type int, true given\n",
            "TypeError:hebrev(): Argument #2 ($max_chars_per_line) must be of type int, string given\n",
            "TypeError:hebrev(): Argument #2 ($max_chars_per_line) must be of type int, null given\n",
        )
    );
}
