mod common;
use common::run_php;

#[test]
fn scalar_string_builtins_preserve_precision_bytes_and_call_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('precision', '12');
$number = 1.23456789012345;
$name = 'strlen';
$first = strlen(...);
echo strlen($number), ':';
echo $name($number), ':';
echo $first($number), ':';
echo call_user_func('strlen', $number), ':';
echo strlen(string: $number), ':';
ini_set('precision', '3');
echo strlen($number), ':', strlen("Ž" . chr(255)), "\n";

$points = [0, 65, 97, 127, 128, 255];
foreach (['strcmp', 'strcasecmp', 'strncmp', 'strncasecmp'] as $function) {
    $matrix = '';
    foreach ($points as $left) {
        foreach ($points as $right) {
            $matrix .= str_starts_with($function, 'strn')
                ? $function(chr($left), chr($right), 1) . ','
                : $function(chr($left), chr($right)) . ',';
        }
    }
    echo $function, '=', md5($matrix), "\n";
}
$compare = strcmp(...);
$left = chr(255);
$reference =& $left;
$copy = $left;
echo strcmp(chr(128), chr(255)), ':';
echo $compare(chr(128), chr(255)), ':';
echo call_user_func('strcmp', chr(128), chr(255)), ':';
echo strcmp(string2: chr(255), string1: chr(128)), ':';
echo strcmp($reference, chr(128)), ':', bin2hex($copy), ':', bin2hex($left), "\n";

class BoundaryBox { public string $edge; }
$box = new BoundaryBox;
$box->edge = chr(127) . chr(128);
$value = [
    'binary' => chr(128) . chr(255),
    'unicode' => "Žluť",
    'nested' => [['edge' => chr(0) . chr(255)]],
    'object' => $box,
];
$returned = print_r($value, true);
ob_start();
$result = print_r($value);
$captured = ob_get_clean();
$edge = strpos($returned, chr(128) . chr(255));
echo strlen($returned), ':', md5($returned), ':', $edge, ':', bin2hex(substr($returned, $edge, 2)), "\n";
echo strlen($captured), ':', md5($captured), ':', ($result ? 'true' : 'false'), "\n";
$copy = $value;
$reference =& $value['binary'];
$reference .= chr(0);
echo md5(print_r($copy, true)), ':', md5(print_r($value, true)), "\n";
"#,
        ),
        concat!(
            "13:13:13:13:13:4:3\n",
            "strcmp=0eb41f8ff77e9b094ea9f2e4ef4ce72a\n",
            "strcasecmp=66ccbd3b171cb461b587627e4e5b9d1e\n",
            "strncmp=0eb41f8ff77e9b094ea9f2e4ef4ce72a\n",
            "strncasecmp=66ccbd3b171cb461b587627e4e5b9d1e\n",
            "-127:-127:-127:-127:127:ff:ff\n",
            "272:26cc729d15e0f38ca4bba7c33bf7e87b:24:80ff\n",
            "272:26cc729d15e0f38ca4bba7c33bf7e87b:true\n",
            "26cc729d15e0f38ca4bba7c33bf7e87b:2a4ee41574b8dd6ddf392df374d0e0ca\n",
        )
    );
}

#[test]
fn scalar_string_builtins_reject_weak_only_inputs_in_strict_callers() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach (
    [
        'strlen' => static fn () => strlen(1.5),
        'strcmp' => static fn () => strcmp(chr(128), 1),
        'strcasecmp' => static fn () => strcasecmp(1, chr(255)),
        'strncmp' => static fn () => strncmp(chr(128), 1, 1),
        'strncasecmp' => static fn () => strncasecmp(1, chr(255), 1),
    ] as $name => $call
) {
    try {
        $call();
    } catch (TypeError $error) {
        echo $name, '=', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "strlen=strlen(): Argument #1 ($string) must be of type string, float given\n",
            "strcmp=strcmp(): Argument #2 ($string2) must be of type string, int given\n",
            "strcasecmp=strcasecmp(): Argument #1 ($string1) must be of type string, int given\n",
            "strncmp=strncmp(): Argument #2 ($string2) must be of type string, int given\n",
            "strncasecmp=strncasecmp(): Argument #1 ($string1) must be of type string, int given\n",
        )
    );
}

#[test]
fn bounded_string_comparisons_validate_arguments_in_signature_order() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function report(callable $call): void {
    try { var_dump($call()); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
report(static fn () => strncmp(1, 2, -1));
report(static fn () => strncmp('a', 2, -1));
report(static fn () => strncmp('a', 'b', -1));
report(static fn () => strncmp('a', 'b', '1'));
"#,
        ),
        concat!(
            "TypeError:strncmp(): Argument #1 ($string1) must be of type string, int given\n",
            "TypeError:strncmp(): Argument #2 ($string2) must be of type string, int given\n",
            "ValueError:strncmp(): Argument #3 ($length) must be greater than or equal to 0\n",
            "TypeError:strncmp(): Argument #3 ($length) must be of type int, string given\n",
        )
    );
}
