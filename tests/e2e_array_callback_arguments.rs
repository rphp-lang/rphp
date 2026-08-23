mod common;

use common::run_php;

#[test]
fn array_filter_supports_null_callbacks_and_value_key_modes() {
    assert_eq!(
        run_php(
            r#"<?php
$values = ['zero' => 0, 'one' => 1, 2 => 2, 'empty' => ''];

$bothSeen = [];
$both = array_filter($values, function ($value, $key) use (&$bothSeen) {
    $bothSeen[] = $key . '=' . $value;
    return $key !== 'zero';
}, ARRAY_FILTER_USE_BOTH);
echo implode(',', $bothSeen), '|', implode(',', array_keys($both)), "\n";

$keySeen = [];
$keys = array_filter($values, function ($key) use (&$keySeen) {
    $keySeen[] = $key;
    return is_int($key);
}, ARRAY_FILTER_USE_KEY);
echo implode(',', $keySeen), '|', implode(',', array_keys($keys)), "\n";

$fallback = array_filter($values, fn ($value) => $value !== 0, 3);
$nullCallback = array_filter($values, null, ARRAY_FILTER_USE_KEY);
echo implode(',', array_keys($fallback)), '|', implode(',', array_keys($nullCallback)), "\n";

$shared = [7];
$references = [&$shared];
$kept = array_filter($references);
$kept[0][] = 8;
echo implode(',', $shared);
"#,
        ),
        "zero=0,one=1,2=2,empty=|one,2,empty\nzero,one,2,empty|2\none,2,empty|one,2\n7,8"
    );
}

#[test]
fn array_walk_passes_userdata_and_commits_before_an_exception() {
    assert_eq!(
        run_php(
            r#"<?php
$values = ['a' => 1, 'b' => 2];
$state = (object) ['step' => 3, 'sum' => 0];
array_walk($values, function (&$value, $key, $userdata) {
    $value += $userdata->step;
    $userdata->sum += $value;
}, $state);
echo implode(',', $values), ':', $state->sum, "\n";

$partial = [1, 2, 3];
try {
    array_walk($partial, function (&$value, $key) {
        $value *= 10;
        if ($key === 1) {
            throw new Exception('stop');
        }
    });
} catch (Exception $exception) {
    echo $exception->getMessage(), ':';
}
echo implode(',', $partial);
"#,
        ),
        "4,5:9\nstop:10,20,3"
    );
}

#[test]
fn recursive_walk_accepts_objects_and_userdata_for_leaf_values() {
    assert_eq!(
        run_php(
            r#"<?php
class WalkBox {
    private $hidden = 1;
    public $nested = ['leaf' => 2];
}

$box = new WalkBox();
$seen = [];
array_walk_recursive($box, function (&$value, $key, $prefix) use (&$seen) {
    $seen[] = $key;
    $value = $prefix . $value;
}, '>');
echo count($seen), ':', end($seen), ':', $box->nested['leaf'], "\n";

$values = ['x' => 1, 'nested' => ['y' => 2]];
array_walk_recursive($values, function (&$value, $key, $prefix) {
    $value = $prefix . $key . '=' . $value;
}, '!');
echo $values['x'], ':', $values['nested']['y'];
"#,
        ),
        "2:leaf:>2\n!x=1:!y=2"
    );
}

#[test]
fn callback_array_functions_expose_names_named_arguments_and_type_errors() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
foreach (['array_filter', 'array_walk', 'array_walk_recursive'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfParameters(), '/',
        $reflection->getNumberOfRequiredParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), $parameter->isPassedByReference() ? '&' : '-', ',';
    }
    echo "\n";
}

$values = ['a' => 1, 'b' => 2];
$filtered = array_filter(
    array: $values,
    callback: fn ($value, $key) => $key === 'b',
    mode: ARRAY_FILTER_USE_BOTH,
);
array_walk(array: $values, callback: function (&$value, $key, $arg) {
    $value += $arg;
}, arg: 2);
echo implode(',', $filtered), '|', implode(',', $values), "\n";

foreach ([
    fn () => array_filter([1], null, '1'),
    fn () => array_filter([1], 'missing_callback'),
    function () { $value = 1; return array_walk($value, fn () => null); },
] as $call) {
    try {
        $call();
    } catch (Throwable $throwable) {
        echo $throwable::class, ': ', $throwable->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "array_filter:3/1:array-,callback-,mode-,\n",
            "array_walk:3/2:array&,callback-,arg-,\n",
            "array_walk_recursive:3/2:array&,callback-,arg-,\n",
            "2|3,4\n",
            "TypeError: array_filter(): Argument #3 ($mode) must be of type int, string given\n",
            "TypeError: array_filter(): Argument #2 ($callback) must be a valid callback or null, function \"missing_callback\" not found or invalid function name\n",
            "TypeError: array_walk(): Argument #1 ($array) must be of type array, int given\n",
        )
    );
}
