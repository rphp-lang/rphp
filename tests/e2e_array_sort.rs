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
fn internal_sorts_match_php_pairwise_mixed_comparisons() {
    assert_eq!(
        run_php(
            r#"<?php
$tags = ['N', 'F', 'T', 'I', 'D', 'E', 'S', 'A'];
$values = [null, false, true, -4, 0.0, '', '-.9', []];
echo 'pairs:';
foreach ($values as $leftIndex => $left) {
    foreach ($values as $rightIndex => $right) {
        $pair = ['L' . $tags[$leftIndex] => $left, 'R' . $tags[$rightIndex] => $right];
        asort($pair, SORT_REGULAR);
        echo array_key_first($pair)[0];
    }
    echo '/';
}
"#,
        ),
        "pairs:LLLLLLLL/LLLLLLLL/RRLLRRLR/RRLLLRLL/LLLRLRRL/LLLLLLLL/RRLRLRLL/LLLRRRRL/"
    );
}

#[test]
fn internal_sorts_match_zend_schedule_boundaries_directions_and_key_policy() {
    assert_eq!(
        run_php(
            r#"<?php
function mixedTag($value): string {
    if ($value === null) return 'N';
    if ($value === false) return 'F';
    if ($value === true) return 'T';
    if (is_int($value)) return 'I';
    if (is_float($value)) return 'D';
    if ($value === '') return 'E';
    if ($value === '-.9') return 'S';
    if (is_array($value)) return 'A';
    return '?';
}

$tags = ['N', 'F', 'T', 'I', 'D', 'E', 'S', 'A'];
$values = [null, false, true, -4, 0.0, '', '-.9', []];
foreach ([5, 6, 10, 16, 17, 22] as $length) {
    $input = [];
    for ($index = 0; $index < $length; $index++) {
        $input[$index . $tags[$index % count($tags)]] = $values[$index % count($values)];
    }
    $ascending = $input;
    asort($ascending, SORT_REGULAR);
    $descending = $input;
    arsort($descending, SORT_REGULAR);
    echo $length, ':', implode(',', array_keys($ascending)), '|',
        implode(',', array_keys($descending)), "\n";
}

$reindexAscending = ['null' => null, 'false' => false, 'negative' => -4, 'zero' => 0.0,
    'empty' => '', 'numeric' => '-.9', 'true' => true, 'array' => []];
$reindexDescending = $reindexAscending;
$preserveAscending = $reindexAscending;
$preserveDescending = $reindexAscending;
sort($reindexAscending, SORT_REGULAR);
rsort($reindexDescending, SORT_REGULAR);
asort($preserveAscending, SORT_REGULAR);
arsort($preserveDescending, SORT_REGULAR);
echo 'keys:', implode(',', array_keys($reindexAscending)), '|',
    implode(',', array_keys($reindexDescending)), '|',
    implode(',', array_keys($preserveAscending)), '|',
    implode(',', array_keys($preserveDescending)), "\n";
echo 'types:', implode('', array_map('mixedTag', $reindexAscending)), '|',
    implode('', array_map('mixedTag', $reindexDescending)), '|',
    implode('', array_map('mixedTag', $preserveAscending)), '|',
    implode('', array_map('mixedTag', $preserveDescending)), "\n";
$keyedAscending = [10 => 'ten', 2 => 'two', 'a' => 'letter', -1 => 'minus', '01' => 'leading'];
$keyedDescending = $keyedAscending;
ksort($keyedAscending, SORT_REGULAR);
krsort($keyedDescending, SORT_REGULAR);
echo 'key-sort:', implode(',', array_keys($keyedAscending)), '|',
    implode(',', array_keys($keyedDescending)), "\n";

$stringCycle = [];
foreach (range(0, 21) as $index) {
    $stringCycle['k' . $index] = ['2', '10', '15a'][$index % 3];
}
asort($stringCycle, SORT_REGULAR);
echo 'string-cycle:', implode(',', array_keys($stringCycle)), '|',
    implode(',', $stringCycle);
"#,
        ),
        concat!(
            "5:0N,1F,2T,3I,4D|2T,3I,0N,1F,4D\n",
            "6:0N,1F,5E,2T,3I,4D|2T,3I,0N,1F,4D,5E\n",
            "10:0N,1F,5E,2T,3I,6S,4D,7A,8N,9F|2T,6S,3I,0N,1F,7A,4D,5E,8N,9F\n",
            "16:0N,1F,5E,2T,3I,6S,4D,7A,8N,9F,13E,10T,11I,14S,12D,15A|",
            "2T,6S,3I,10T,0N,1F,7A,4D,14S,11I,5E,8N,9F,15A,12D,13E\n",
            "17:0N,1F,5E,4D,7A,8N,9F,13E,2T,3I,6S,10T,11I,14S,12D,15A,16N|",
            "2T,6S,3I,10T,11I,0N,1F,7A,4D,14S,5E,8N,9F,15A,12D,13E,16N\n",
            "22:0N,1F,5E,8N,9F,13E,16N,17F,21E,3I,6S,4D,12D,7A,10T,11I,14S,15A,2T,18T,19I,20D|",
            "2T,6S,14S,3I,10T,11I,18T,19I,0N,1F,7A,4D,8N,9F,15A,12D,5E,13E,16N,17F,20D,21E\n",
            "keys:0,1,2,3,4,5,6,7|0,1,2,3,4,5,6,7|",
            "null,false,empty,negative,numeric,zero,array,true|",
            "negative,true,null,false,array,zero,numeric,empty\n",
            "types:NFEISDAT|ITNFADSE|NFEISDAT|ITNFADSE\n",
            "key-sort:-1,01,2,10,a|a,10,2,01,-1\n",
            "string-cycle:k2,k5,k8,k11,k14,k17,k20,k0,k3,k6,k9,k12,k15,k18,k21,",
            "k1,k4,k7,k10,k13,k16,k19|15a,15a,15a,15a,15a,15a,15a,",
            "2,2,2,2,2,2,2,2,10,10,10,10,10,10,10",
        )
    );
}

#[test]
fn array_multisort_matches_zend_numeric_warning_schedule_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
class NumericA {}
class NumericB {}
class NumericC {}

$warningTrace = [];
$warningCount = 0;
$warningDigest = 0;
$recordWarningTrace = true;
function numericWarning($level, $message): bool {
    global $warningTrace, $warningCount, $warningDigest, $recordWarningTrace;
    foreach (['NumericA' => ['A', 1], 'NumericB' => ['B', 2], 'NumericC' => ['C', 3]] as $class => $tag) {
        if (str_contains($message, $class)) {
            if ($recordWarningTrace) $warningTrace[] = $tag[0];
            $warningCount++;
            $warningDigest = ($warningDigest * 131 + $tag[1]) % 2147483647;
            break;
        }
    }
    return true;
}

foreach ([2, 3, 4, 5, 6, 10, 16, 17] as $length) {
    $objects = [];
    for ($index = 0; $index < $length; $index++) {
        $objects[] = match ($index % 3) {
            0 => new NumericA(),
            1 => new NumericB(),
            default => new NumericC(),
        };
    }
    $warningTrace = [];
    $warningCount = 0;
    $warningDigest = 0;
    $recordWarningTrace = true;
    set_error_handler('numericWarning');
    array_multisort($objects, SORT_ASC, SORT_NUMERIC);
    restore_error_handler();
    $order = '';
    foreach ($objects as $object) {
        $order .= substr(get_class($object), -1);
    }
    echo 'warnings-', $length, ':', implode('', $warningTrace), '|', $order, "\n";
}

foreach ([1023, 1024] as $length) {
    $objects = [];
    for ($index = 0; $index < $length; $index++) {
        $objects[] = match ($index % 3) {
            0 => new NumericA(),
            1 => new NumericB(),
            default => new NumericC(),
        };
    }
    $warningTrace = [];
    $warningCount = 0;
    $warningDigest = 0;
    $recordWarningTrace = false;
    set_error_handler('numericWarning');
    array_multisort($objects, SORT_ASC, SORT_NUMERIC);
    restore_error_handler();
    $orderDigest = 0;
    foreach ($objects as $object) {
        $tag = match (get_class($object)) {
            'NumericA' => 1,
            'NumericB' => 2,
            default => 3,
        };
        $orderDigest = ($orderDigest * 131 + $tag) % 2147483647;
    }
    echo 'pivot-', $length, ':', $warningCount, ':', $warningDigest, '|', $orderDigest, "\n";
}
"#,
        ),
        concat!(
            "warnings-2:AB|AB\n",
            "warnings-3:ABBC|ABC\n",
            "warnings-4:ABBCCA|ABCA\n",
            "warnings-5:ABBCCAAB|ABCAB\n",
            "warnings-6:ABBCCAABBC|ABCABC\n",
            "warnings-10:ABBCCAABBCCAABBCCA|ABCABCABCA\n",
            "warnings-16:ABBCCAABBCCAABBCCAABBCCAABBCCA|ABCABCABCABCABCA\n",
            "warnings-17:ACCBCCCACBCCCACBCBCAACCCBCACCCBC",
            "ABBCCAABBCCAABABBCCAABBCCAAB|ABCABCABCABCABCAB\n",
            "pivot-1023:13828:1671710759|1999957511\n",
            "pivot-1024:13846:907007518|1429008\n",
        )
    );
}

#[test]
fn array_multisort_matches_array_object_numeric_and_string_diagnostic_order() {
    assert_eq!(
        run_php(
            r#"<?php
class DiagnosticStringable {
    public function __toString(): string { return 'A'; }
}
class DiagnosticOpaque {}

foreach ([SORT_NUMERIC, SORT_STRING] as $flag) {
    echo 'flag=', $flag, "\n";
    $warnings = [];
    set_error_handler(function ($level, $message) use (&$warnings) {
        $warnings[] = $message;
        return true;
    });
    $values = [[1], new DiagnosticStringable(), [2], new DiagnosticOpaque()];
    try {
        array_multisort($values, SORT_ASC, $flag);
        echo "ok\n";
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
    restore_error_handler();
    echo count($warnings), "\n", implode("\n", $warnings), "\n";
}
"#,
        ),
        concat!(
            "flag=1\n",
            "ok\n",
            "3\n",
            "Object of class DiagnosticStringable could not be converted to float\n",
            "Object of class DiagnosticStringable could not be converted to float\n",
            "Object of class DiagnosticOpaque could not be converted to float\n",
            "flag=2\n",
            "Error:Object of class DiagnosticOpaque could not be converted to string\n",
            "5\n",
            "Array to string conversion\n",
            "Array to string conversion\n",
            "Array to string conversion\n",
            "Array to string conversion\n",
            "Array to string conversion\n",
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
fn user_sorts_match_php_small_input_traces_stability_and_exception_stop() {
    assert_eq!(
        run_php(
            r#"<?php
function traceUserSort($function, $values) {
    $trace = [];
    $array = $values;
    $function($array, function ($left, $right) use (&$trace) {
        $trace[] = "$left:$right";
        return $left <=> $right;
    });
    $order = $function === 'uksort' ? array_keys($array) : $array;
    echo $function, '|', count($values), '|', implode(',', $trace), '|', implode(',', $order), "\n";
}

foreach (['usort', 'uasort'] as $function) {
    foreach ([[2, 1], [3, 2, 1], [4, 3, 2, 1], [5, 4, 3, 2, 1], [1, 2, 3, 4, 5]] as $values) {
        traceUserSort($function, $values);
    }
}
foreach ([
    [2 => 0, 1 => 0],
    [3 => 0, 2 => 0, 1 => 0],
    [4 => 0, 3 => 0, 2 => 0, 1 => 0],
    [5 => 0, 4 => 0, 3 => 0, 2 => 0, 1 => 0],
    [1 => 0, 2 => 0, 3 => 0, 4 => 0, 5 => 0],
] as $values) {
    traceUserSort('uksort', $values);
}

foreach (['usort', 'uasort', 'uksort'] as $function) {
    $trace = [];
    if ($function === 'uksort') {
        $array = ['a' => 0, 'b' => 0, 'c' => 0, 'd' => 0, 'e' => 0];
        $callback = function ($left, $right) use (&$trace) {
            $trace[] = "$left:$right";
            return 0;
        };
    } else {
        $array = [];
        foreach (['a', 'b', 'c', 'd', 'e'] as $id) {
            $value = new stdClass;
            $value->id = $id;
            $array[] = $value;
        }
        $callback = function ($left, $right) use (&$trace) {
            $trace[] = "$left->id:$right->id";
            return 0;
        };
    }
    $function($array, $callback);
    $order = $function === 'uksort'
        ? array_keys($array)
        : array_map(fn($value) => $value->id, $array);
    echo "tie:$function|", implode(',', $trace), '|', implode(',', $order), "\n";
}

foreach (['usort', 'uasort', 'uksort'] as $function) {
    $calls = 0;
    $array = $function === 'uksort'
        ? [4 => 0, 3 => 0, 2 => 0, 1 => 0]
        : [4, 3, 2, 1];
    try {
        $function($array, function ($left, $right) use (&$calls) {
            if (++$calls === 2) {
                throw new RuntimeException('stop');
            }
            return $left <=> $right;
        });
    } catch (RuntimeException $error) {
        echo "throw:$function|", $error->getMessage(), '|', $calls, "\n";
    }
}
"#,
        ),
        concat!(
            "usort|2|2:1|1,2\n",
            "usort|3|3:2,1:2|1,2,3\n",
            "usort|4|4:3,2:3,4:1,3:1,2:1|1,2,3,4\n",
            "usort|5|5:4,3:4,5:2,4:2,3:2,5:1,4:1,3:1,2:1|1,2,3,4,5\n",
            "usort|5|1:2,2:3,3:4,4:5|1,2,3,4,5\n",
            "uasort|2|2:1|1,2\n",
            "uasort|3|3:2,1:2|1,2,3\n",
            "uasort|4|4:3,2:3,4:1,3:1,2:1|1,2,3,4\n",
            "uasort|5|5:4,3:4,5:2,4:2,3:2,5:1,4:1,3:1,2:1|1,2,3,4,5\n",
            "uasort|5|1:2,2:3,3:4,4:5|1,2,3,4,5\n",
            "uksort|2|2:1|1,2\n",
            "uksort|3|3:2,1:2|1,2,3\n",
            "uksort|4|4:3,2:3,4:1,3:1,2:1|1,2,3,4\n",
            "uksort|5|5:4,3:4,5:2,4:2,3:2,5:1,4:1,3:1,2:1|1,2,3,4,5\n",
            "uksort|5|1:2,2:3,3:4,4:5|1,2,3,4,5\n",
            "tie:usort|a:b,b:c,c:d,d:e|a,b,c,d,e\n",
            "tie:uasort|a:b,b:c,c:d,d:e|a,b,c,d,e\n",
            "tie:uksort|a:b,b:c,c:d,d:e|a,b,c,d,e\n",
            "throw:usort|stop|2\n",
            "throw:uasort|stop|2\n",
            "throw:uksort|stop|2\n",
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
