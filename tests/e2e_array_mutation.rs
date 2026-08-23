mod common;

use common::run_php;

#[test]
fn array_push_and_unshift_match_php_85_variadics_keys_and_value_semantics() {
    assert_eq!(
        run_php(
            r#"<?php
function signature(string $name): void {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), ',',
            $parameter->isVariadic() ? 'variadic' : 'fixed', ',',
            $parameter->isPassedByReference() ? 'ref' : 'value', ';';
    }
    echo "\n";
}

signature('array_push');
signature('array_unshift');

$array = [2 => 'two', 'name' => 'value'];
echo 'push-zero:', array_push($array), ':', json_encode($array), "\n";
echo 'push-many:', array_push($array, 'a', 'b'), ':', json_encode($array), "\n";

$slot = 10;
$alias = &$slot;
$array = [];
array_push($array, $alias);
$slot = 20;
$array[0] = 30;
echo 'push-detached:', $slot, "\n";

$nested = ['v' => 1];
$object = (object) ['v' => 1];
$array = [];
array_push($array, $nested, $object);
$array[0]['v'] = 2;
$array[1]->v = 2;
echo 'push-cow-object:', $nested['v'], ':', $object->v, "\n";

$array = [2 => 'two', 'name' => 'value'];
echo 'unshift-zero:', array_unshift($array), ':', json_encode($array), "\n";
echo 'unshift-many:', array_unshift($array, 'a', 'b'), ':', json_encode($array), "\n";

$slot = 40;
$alias = &$slot;
$array = ['tail'];
array_unshift($array, $alias);
$slot = 41;
$array[0] = 42;
echo 'unshift-detached:', $slot, "\n";
"#,
        ),
        concat!(
            "array_push:1/2:array,fixed,ref;values,variadic,value;\n",
            "array_unshift:1/2:array,fixed,ref;values,variadic,value;\n",
            "push-zero:2:{\"2\":\"two\",\"name\":\"value\"}\n",
            "push-many:4:{\"2\":\"two\",\"name\":\"value\",\"3\":\"a\",\"4\":\"b\"}\n",
            "push-detached:20\n",
            "push-cow-object:1:2\n",
            "unshift-zero:2:{\"0\":\"two\",\"name\":\"value\"}\n",
            "unshift-many:4:{\"0\":\"a\",\"1\":\"b\",\"2\":\"two\",\"name\":\"value\"}\n",
            "unshift-detached:41\n",
        )
    );
}

#[test]
fn array_pop_and_shift_match_php_85_references_empty_arrays_and_cursors() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['array_pop', 'array_shift'] as $name) {
    $reflection = new ReflectionFunction($name);
    $parameter = $reflection->getParameters()[0];
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':', $parameter->getName(), ':',
        $parameter->isPassedByReference() ? 'ref' : 'value', "\n";
}

$slot = 50;
$array = ['first', 'ref' => &$slot, 9 => 'last'];
$popped = array_pop($array);
echo 'pop:', json_encode($array), ':', $popped, ':', $slot, "\n";
$popped = 51;
echo 'pop-detached:', $slot, "\n";

$array = ['first', 'ref' => &$slot, 9 => 'last'];
$shifted = array_shift($array);
echo 'shift:', json_encode($array), ':', $shifted, ':', $slot, "\n";

$array = [1, 2, 3];
next($array);
next($array);
array_pop($array);
echo 'pop-cursor:', current($array), "\n";
$array = [1, 2, 3];
next($array);
array_shift($array);
echo 'shift-cursor:', current($array), "\n";
$array = [1, 2, 3];
next($array);
array_unshift($array, 0);
echo 'unshift-cursor:', current($array), "\n";

$empty = [];
var_dump(array_pop($empty), array_shift($empty));

$negative = [-2 => false];
array_pop($negative);
$negative[] = true;
$negative[] = true;
$negative[] = true;
echo 'negative-pop:', json_encode($negative), "\n";
"#,
        ),
        concat!(
            "array_pop:1/1:array:ref\n",
            "array_shift:1/1:array:ref\n",
            "pop:{\"0\":\"first\",\"ref\":50}:last:50\n",
            "pop-detached:50\n",
            "shift:{\"ref\":50,\"0\":\"last\"}:first:50\n",
            "pop-cursor:1\n",
            "shift-cursor:2\n",
            "unshift-cursor:0\n",
            "NULL\n",
            "NULL\n",
            "negative-pop:{\"-2\":true,\"-1\":true,\"0\":true}\n",
        )
    );
}

#[test]
fn array_splice_matches_php_85_bounds_keys_replacements_references_and_cow() {
    assert_eq!(
        run_php(
            r#"<?php
$reflection = new ReflectionFunction('array_splice');
echo 'signature:', $reflection->getNumberOfRequiredParameters(), '/',
    $reflection->getNumberOfParameters(), ':';
foreach ($reflection->getParameters() as $parameter) {
    echo $parameter->getName(), ',',
        $parameter->isPassedByReference() ? 'ref' : 'value', ';';
}
echo "\n";

foreach ([[1, -1], [1, -2], [-2, 1], [-9, PHP_INT_MAX], [PHP_INT_MAX, PHP_INT_MIN]] as [$offset, $length]) {
    $array = [0, 1, 2, 3, 4];
    $removed = array_splice($array, $offset, $length, ['x', 'y']);
    echo $offset, ':', $length, ':', json_encode($removed), ':', json_encode($array), "\n";
}

$array = [4 => 'four', 'keep' => 'value', 9 => 'nine', 'drop' => 'gone'];
$removed = array_splice($array, 1, 2, ['named' => 'replacement']);
echo 'keys:', json_encode($removed), ':', json_encode($array), "\n";

foreach ([null, 'scalar', 7, (object) ['a' => 'first', 'b' => 'second']] as $replacement) {
    $array = [0, 1, 2];
    $removed = array_splice($array, 1, 1, $replacement);
    echo get_debug_type($replacement), ':', json_encode($removed), ':', json_encode($array), "\n";
}

$slot = 60;
$array = [0, &$slot, 2];
$replacement = [&$slot];
$removed = array_splice($array, 1, 1, $replacement);
$slot = 61;
echo 'refs:', $array[1], ':', $removed[0], ':', $slot, "\n";
$array[1] = 62;
echo 'refs-live:', $removed[0], ':', $slot, "\n";

$nested = ['v' => 1];
$array = [$nested, 2];
$removed = array_splice($array, 0, 1, [$nested]);
$array[0]['v'] = 2;
$removed[0]['v'] = 3;
echo 'cow:', $nested['v'], ':', $array[0]['v'], ':', $removed[0]['v'], "\n";
"#,
        ),
        concat!(
            "signature:2/4:array,ref;offset,value;length,value;replacement,value;\n",
            "1:-1:[1,2,3]:[0,\"x\",\"y\",4]\n",
            "1:-2:[1,2]:[0,\"x\",\"y\",3,4]\n",
            "-2:1:[3]:[0,1,2,\"x\",\"y\",4]\n",
            "-9:9223372036854775807:[0,1,2,3,4]:[\"x\",\"y\"]\n",
            "9223372036854775807:-9223372036854775808:[]:[0,1,2,3,4,\"x\",\"y\"]\n",
            "keys:{\"keep\":\"value\",\"0\":\"nine\"}:{\"0\":\"four\",\"1\":\"replacement\",\"drop\":\"gone\"}\n",
            "null:[1]:[0,2]\n",
            "string:[1]:[0,\"scalar\",2]\n",
            "int:[1]:[0,7,2]\n",
            "stdClass:[1]:[0,\"first\",\"second\",2]\n",
            "refs:61:61:61\n",
            "refs-live:62:62\n",
            "cow:1:2:3\n",
        )
    );
}

#[test]
fn array_mutators_match_php_85_errors_overflow_and_reentrant_destructors() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['array_push', 'array_pop', 'array_shift', 'array_unshift', 'array_splice'] as $name) {
    $invalid = 1;
    try {
        match ($name) {
            'array_push' => array_push($invalid, 2),
            'array_pop' => array_pop($invalid),
            'array_shift' => array_shift($invalid),
            'array_unshift' => array_unshift($invalid, 2),
            'array_splice' => array_splice($invalid, 0),
        };
    } catch (Throwable $error) {
        echo $name, ':', get_class($error), ':', $error->getMessage(), "\n";
    }
}

$array = array_fill(PHP_INT_MAX, 1, 'edge');
try { array_push($array, 'overflow'); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }

class MutationDestructor {
    public function __destruct() {
        global $destructorArray;
        $destructorArray[] = 'changed';
    }
}
$destructorArray = [new MutationDestructor(), 'tail'];
try { array_splice($destructorArray, 0, 1); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "array_push:TypeError:array_push(): Argument #1 ($array) must be of type array, int given\n",
            "array_pop:TypeError:array_pop(): Argument #1 ($array) must be of type array, int given\n",
            "array_shift:TypeError:array_shift(): Argument #1 ($array) must be of type array, int given\n",
            "array_unshift:TypeError:array_unshift(): Argument #1 ($array) must be of type array, int given\n",
            "array_splice:TypeError:array_splice(): Argument #1 ($array) must be of type array, int given\n",
            "Error:Cannot add element to the array as the next element is already occupied\n",
            "Error:Array was modified during array_splice operation\n",
        )
    );
}

#[test]
fn structural_array_mutators_preserve_live_by_reference_foreach_positions() {
    assert_eq!(
        run_php(
            r#"<?php
$array = ['a', 'b', 'c'];
foreach ($array as &$value) {
    echo $value;
    if ($value === 'b') { array_unshift($array, 'x', 'y'); }
}
echo ':', count($array), "\n";

$array = [10, 20, 30];
foreach ($array as &$value) {
    echo $value, ';';
    array_shift($array);
}
echo json_encode($array), "\n";

$array = [0, 1, 2, 3, 4, 5];
$changed = false;
foreach ($array as &$value) {
    echo $value, ';';
    if (!$changed && $value === 3) {
        $changed = true;
        array_splice($array, 1, 2, ['left', 'right', 'middle']);
    }
}
echo json_encode($array), "\n";

$array = [0, 1, 2, 3, 4];
$changed = false;
foreach ($array as &$value) {
    echo $value, ';';
    if (!$changed && $value === 2) {
        $changed = true;
        array_splice($array, 1, 3, ['replacement']);
    }
}
echo json_encode($array), "\n";
"#,
        ),
        concat!(
            "abc:5\n",
            "10;20;30;[]\n",
            "0;1;2;3;4;5;[0,\"left\",\"right\",\"middle\",3,4,5]\n",
            "0;1;2;4;[0,\"replacement\",4]\n",
        )
    );
}
