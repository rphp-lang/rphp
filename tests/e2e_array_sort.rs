mod common;

use common::run_php;

#[test]
fn ordinary_sorts_apply_flags_stability_keys_reindexing_and_references() {
    assert_eq!(
        run_php(
            r#"<?php
$values = ['ten' => '10', 'two' => '2', 'again' => '2'];
$sorted = $values;
sort($sorted, SORT_NUMERIC);
echo json_encode($sorted), '|';

$reversed = ['a', 'B', 'b'];
rsort($reversed, SORT_STRING | SORT_FLAG_CASE);
echo json_encode($reversed), '|';

$ascending = ['first' => 2, 'second' => 1, 'third' => 1];
asort($ascending, SORT_REGULAR);
echo implode(',', array_keys($ascending)), '|';

$descending = ['first' => 2, 'second' => 1, 'third' => 1];
arsort($descending, SORT_REGULAR);
echo implode(',', array_keys($descending)), '|';

$keys = ['item10' => 10, 'item2' => 2, 'Item1' => 1, 7 => 0];
ksort($keys, SORT_NATURAL | SORT_FLAG_CASE);
echo implode(',', array_keys($keys)), '|';
krsort($keys, SORT_NATURAL | SORT_FLAG_CASE);
echo implode(',', array_keys($keys)), "\n";

$x = 30;
$y = 10;
$z = 20;
$references = [&$x, &$y, &$z];
sort($references, SORT_NUMERIC);
$references[0] = 11;
$references[2] = 31;
echo "$x,$y,$z|";
$references = ['x' => &$x, 'y' => &$y, 'z' => &$z];
arsort($references, SORT_NUMERIC);
$references['z'] = 22;
echo implode(',', array_keys($references)), ":$x,$y,$z\n";

$large = range(1100, 0);
sort($large, SORT_NUMERIC);
echo $large[0], ',', $large[1100], ',', count($large), '|';
rsort($large, SORT_NUMERIC);
echo $large[0], ',', $large[1100];
"#,
        ),
        concat!(
            "[\"2\",\"2\",\"10\"]|[\"B\",\"b\",\"a\"]|",
            "second,third,first|first,second,third|",
            "7,Item1,item2,item10|item10,item2,Item1,7\n",
            "31,11,20|x,z,y:31,11,22\n",
            "0,1100,1101|1100,0",
        )
    );
}

#[test]
fn array_multisort_matches_columns_key_rebuilding_flag_errors_and_numeric_diagnostics() {
    assert_eq!(
        run_php(
            r#"<?php
class SortStringable {
    public function __toString(): string { return 'Class A object'; }
}
class SortOpaque {}
$primary = ['left' => 2, 7 => 1, 'right' => 1];
$secondary = ['left' => 'b', 7 => 'c', 'right' => 'a'];
array_multisort(
    $primary,
    SORT_ASC,
    SORT_NUMERIC,
    $secondary,
    SORT_ASC,
    SORT_STRING,
);
echo json_encode($primary), '|', json_encode($secondary), "\n";

foreach ([0, 1] as $case) {
    $array = [2, 1];
    try {
        if ($case === 0) {
            array_multisort($array, SORT_ASC, SORT_DESC);
        } else {
            array_multisort($array, SORT_STRING, SORT_NUMERIC);
        }
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}

$warnings = [];
set_error_handler(function ($level, $message) use (&$warnings) {
    $warnings[] = $message;
    return true;
});
$numeric = [new SortStringable(), new SortOpaque()];
array_multisort($numeric, SORT_ASC, SORT_NUMERIC);
restore_error_handler();
echo count($warnings), ':', implode(';', $warnings);
"#,
        ),
        concat!(
            "{\"right\":1,\"0\":1,\"left\":2}|{\"right\":\"a\",\"0\":\"c\",\"left\":\"b\"}\n",
            "TypeError:array_multisort(): Argument #3 must be an array or a sort flag that has not already been specified\n",
            "TypeError:array_multisort(): Argument #3 must be an array or a sort flag that has not already been specified\n",
            "2:Object of class SortStringable could not be converted to float;",
            "Object of class SortOpaque could not be converted to float",
        )
    );
}

#[test]
fn array_multisort_classifies_fixed_and_variadic_arguments_before_sorting() {
    assert_eq!(
        run_php(
            r#"<?php
function showFailure($callback) {
    try { $callback(); }
    catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}

showFailure(fn() => array_multisort(0));
showFailure(fn() => array_multisort(12345));
showFailure(fn() => array_multisort(null));
$values = [2, 1];
showFailure(fn() => array_multisort($values, SORT_ASC, SORT_DESC));
$values = [2, 1];
showFailure(fn() => array_multisort($values, SORT_STRING, SORT_NUMERIC));
$values = [2, 1];
showFailure(fn() => array_multisort($values, 12345));
$values = [2, 1];
showFailure(fn() => array_multisort($values, 1.5));

$first = [2, 1];
$second = ['b', 'a'];
$alias =& $second;
var_dump(array_multisort(
    $first,
    SORT_ASC,
    SORT_NUMERIC,
    $alias,
    SORT_DESC,
    SORT_STRING,
));
echo implode(',', $first), '|', implode(',', $second), '|';
$alias[0] = 'x';
echo implode(',', $second);
"#,
        ),
        concat!(
            "TypeError:array_multisort(): Argument #1 ($array) must be an array or a sort flag that has not already been specified\n",
            "ValueError:array_multisort(): Argument #1 ($array) must be a valid sort flag\n",
            "TypeError:array_multisort(): Argument #1 ($array) must be an array or a sort flag\n",
            "TypeError:array_multisort(): Argument #3 must be an array or a sort flag that has not already been specified\n",
            "TypeError:array_multisort(): Argument #3 must be an array or a sort flag that has not already been specified\n",
            "ValueError:array_multisort(): Argument #2 must be a valid sort flag\n",
            "TypeError:array_multisort(): Argument #2 must be an array or a sort flag\n",
            "bool(true)\n",
            "1,2|a,b|x,b",
        )
    );
}

#[test]
fn array_multisort_nested_diagnostics_keep_outer_small_sort_state() {
    assert_eq!(
        run_php(
            r#"<?php
$nested = [];
function nestedMultisortHandler($level, $message) {
    global $nested;
    $missing = null;
    try { array_multisort($missing, SORT_ASC); }
    catch (Throwable $error) {
        $nested[] = get_class($error) . ':' . $error->getMessage();
    }
    return true;
}
set_error_handler('nestedMultisortHandler');
$first = new stdClass;
$second = new stdClass;
$third = new stdClass;
$objects = [$first, $second, $third];
$result = array_multisort($objects, SORT_NUMERIC);
restore_error_handler();
echo (int) $result, '|', count($nested), '|';
echo (int) ($objects[0] === $first), (int) ($objects[1] === $second), (int) ($objects[2] === $third), "\n";
echo implode("\n", $nested);
"#,
        ),
        concat!(
            "1|4|111\n",
            "TypeError:array_multisort(): Argument #1 ($array) must be an array or a sort flag\n",
            "TypeError:array_multisort(): Argument #1 ($array) must be an array or a sort flag\n",
            "TypeError:array_multisort(): Argument #1 ($array) must be an array or a sort flag\n",
            "TypeError:array_multisort(): Argument #1 ($array) must be an array or a sort flag",
        )
    );
}

#[test]
fn recursive_regular_sort_raises_a_catchable_error_without_host_failure() {
    assert_eq!(
        run_php(
            r#"<?php
$left = [];
$left[] = &$left;
$right = [];
$right[] = &$right;
$recursive = [$left, $right];
try {
    sort($recursive);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        "Error:Nesting level too deep - recursive dependency?"
    );
}

#[test]
fn user_sorts_preserve_reference_cells_and_pass_temporary_callback_values() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
$messages = [];
set_error_handler(function ($level, $message) use (&$messages) {
    $messages[] = "$level:$message";
    return true;
});
function hardComparator(&$left, &$right) { return $left <=> $right; }

$x = 30;
$y = 10;
$z = 20;
$indexed = [&$x, &$y, &$z];
usort($indexed, static fn($left, $right) => $left <=> $right);
$indexed[0] = 11;
$indexed[2] = 31;
echo "$x,$y,$z|";

$keyed = ['x' => &$x, 'y' => &$y, 'z' => &$z];
uasort($keyed, static fn($left, $right) => $left <=> $right);
$keyed['z'] = 22;
echo implode(',', array_keys($keyed)), ":$x,$y,$z|";

$indexed = [2, 1];
usort($indexed, 'hardComparator');
$keyed = ['two' => 2, 'one' => 1];
uasort($keyed, 'hardComparator');
$keys = ['b' => 2, 'a' => 1];
uksort($keys, 'hardComparator');
restore_error_handler();
echo count($messages), "\n", implode("\n", $messages);
"#,
        ),
        concat!(
            "31,11,20|y,z,x:31,11,22|6\n",
            "2:hardComparator(): Argument #1 ($left) must be passed by reference, value given\n",
            "2:hardComparator(): Argument #2 ($right) must be passed by reference, value given\n",
            "2:hardComparator(): Argument #1 ($left) must be passed by reference, value given\n",
            "2:hardComparator(): Argument #2 ($right) must be passed by reference, value given\n",
            "2:hardComparator(): Argument #1 ($left) must be passed by reference, value given\n",
            "2:hardComparator(): Argument #2 ($right) must be passed by reference, value given",
        )
    );
}

#[test]
fn user_sorts_deprecate_boolean_results_once_and_reverse_false_comparisons() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
$messages = [];
$trace = [];
set_error_handler(function ($level, $message) use (&$messages) {
    $messages[] = "$level:$message";
    return true;
});
function legacyBooleanCompare($left, $right) {
    global $trace;
    $trace[] = "$left:$right";
    return $left > $right;
}

$indexed = [1, 2];
usort($indexed, 'legacyBooleanCompare');
echo implode(',', $indexed), '|', implode(',', $trace), "\n";

$trace = [];
$keyed = ['left' => 1, 'right' => 2];
uasort($keyed, 'legacyBooleanCompare');
echo implode(',', array_keys($keyed)), '|', implode(',', $trace), "\n";

$trace = [];
$keys = ['a' => 1, 'b' => 2];
uksort($keys, 'legacyBooleanCompare');
echo implode(',', array_keys($keys)), '|', implode(',', $trace), "\n";
restore_error_handler();
echo count($messages), "\n", implode("\n", $messages);
"#,
        ),
        concat!(
            "1,2|1:2,2:1\n",
            "left,right|1:2,2:1\n",
            "a,b|a:b,b:a\n",
            "3\n",
            "8192:usort(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero\n",
            "8192:uasort(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero\n",
            "8192:uksort(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero",
        )
    );
}
