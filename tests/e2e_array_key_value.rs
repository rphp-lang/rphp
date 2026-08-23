mod common;

use common::run_php;

#[test]
fn array_keys_and_values_match_php_85_filters_references_and_cow() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['array_keys', 'array_values'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), ';';
    }
    echo "\n";
}

$source = [
    'int' => 1,
    'string' => '1',
    'null' => null,
    'false' => false,
    'zero' => 0,
    'string-zero' => '0',
    'empty' => '',
    'array' => [],
];
echo 'keys-loose-one:', json_encode(array_keys($source, 1)), "\n";
echo 'keys-strict-one:', json_encode(array_keys($source, 1, true)), "\n";
echo 'keys-loose-null:', json_encode(array_keys($source, null)), "\n";
echo 'keys-strict-null:', json_encode(array_keys($source, null, true)), "\n";
echo 'keys-loose-string-zero:', json_encode(array_keys($source, '0')), "\n";

$slot = 'one';
$nested = ['value' => 1];
$object = (object) ['value' => 1];
$source = ['reference' => &$slot, 'nested' => $nested, 'object' => $object];
$values = array_values($source);
$slot = 'two';
$values[1]['value'] = 2;
$values[2]->value = 2;
echo 'values:', $values[0], ':', $source['nested']['value'], ':',
    $nested['value'], ':', $source['object']->value, "\n";
$values[0] = 'three';
echo 'values-reference:', $slot, "\n";

$detached = 'only-owner';
$singleOwner = [&$detached];
unset($detached);
$projected = array_values($singleOwner);
ob_start();
var_dump($projected);
$dump = ob_get_clean();
echo 'values-detached-reference:', str_contains($dump, '&') ? 'wrapped' : 'value', "\n";
"#,
        ),
        concat!(
            "array_keys:1/3:array;filter_value;strict;\n",
            "array_values:1/1:array;\n",
            "keys-loose-one:[\"int\",\"string\"]\n",
            "keys-strict-one:[\"int\"]\n",
            "keys-loose-null:[\"null\",\"false\",\"zero\",\"empty\",\"array\"]\n",
            "keys-strict-null:[\"null\"]\n",
            "keys-loose-string-zero:[\"false\",\"zero\",\"string-zero\"]\n",
            "values:two:1:1:2\n",
            "values-reference:three\n",
            "values-detached-reference:value\n",
        )
    );
}

#[test]
fn array_flip_and_count_values_match_php_85_keys_warnings_and_reentrancy() {
    assert_eq!(
        run_php(
            r#"<?php
$warnings = [];
set_error_handler(function ($level, $message) use (&$warnings) {
    $warnings[] = $message;
    return true;
});
$flipped = array_flip([
    'first' => '1',
    'replacement' => 1,
    'padded' => '01',
    'negative' => '-2',
    'negative-replacement' => -2,
    'bad-bool' => true,
    'bad-array' => [],
]);
$counted = array_count_values(['1', 1, '01', '-2', -2, true, [], null]);
restore_error_handler();
echo 'flip:', json_encode($flipped), "\n";
echo 'count:', json_encode($counted), "\n";
foreach ($warnings as $warning) {
    echo 'warning:', $warning, "\n";
}

set_error_handler(function ($level, $message) {
    throw new RuntimeException('warning-stop');
});
try {
    array_flip(['accepted' => 1, 'rejected' => false, 'unreached' => 2]);
} catch (Throwable $error) {
    echo 'throwing-handler:', get_class($error), ':', $error->getMessage(), "\n";
}
restore_error_handler();
"#,
        ),
        concat!(
            "flip:{\"1\":\"replacement\",\"01\":\"padded\",\"-2\":\"negative-replacement\"}\n",
            "count:{\"1\":2,\"01\":1,\"-2\":2}\n",
            "warning:array_flip(): Can only flip string and integer values, entry skipped\n",
            "warning:array_flip(): Can only flip string and integer values, entry skipped\n",
            "warning:array_count_values(): Can only count string and integer values, entry skipped\n",
            "warning:array_count_values(): Can only count string and integer values, entry skipped\n",
            "warning:array_count_values(): Can only count string and integer values, entry skipped\n",
            "throwing-handler:RuntimeException:warning-stop\n",
        )
    );
}

#[test]
fn array_rand_matches_php_85_cardinality_order_and_error_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$reflection = new ReflectionFunction('array_rand');
echo 'signature:', $reflection->getNumberOfRequiredParameters(), '/',
    $reflection->getNumberOfParameters(), ':';
foreach ($reflection->getParameters() as $parameter) {
    echo $parameter->getName(), ';';
}
echo "\n";

function key_position(mixed $key): int {
    if ($key === 9) return 0;
    if ($key === 'a') return 1;
    if ($key === 3) return 2;
    if ($key === 'b') return 3;
    if ($key === 1) return 4;
    if ($key === 'c') return 5;
    return -1;
}

$input = [9 => 'nine', 'a' => 'a', 3 => 'three', 'b' => 'b', 1 => 'one', 'c' => 'c'];
echo 'all:', json_encode(array_rand($input, 6)), "\n";
$valid = true;
for ($iteration = 0; $iteration < 128; $iteration++) {
    $picked = array_rand($input, 4);
    if (count($picked) !== 4) {
        $valid = false;
        break;
    }
    $last = -1;
    $seen = [];
    foreach ($picked as $key) {
        $position = key_position($key);
        $tag = get_debug_type($key) . ':' . $key;
        if ($position <= $last || isset($seen[$tag])) {
            $valid = false;
            break 2;
        }
        $seen[$tag] = true;
        $last = $position;
    }
}
echo 'subset-invariants:', $valid ? 'ok' : 'broken', "\n";
echo 'single-invariant:', key_position(array_rand($input)) >= 0 ? 'ok' : 'broken', "\n";
echo 'weak-int:', key_position(array_rand($input, '1')) >= 0 ? 'ok' : 'broken', "\n";

foreach ([[[], 1], [[], 0], [$input, 0], [$input, 7]] as [$array, $num]) {
    try {
        array_rand($array, $num);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "signature:1/2:array;num;\n",
            "all:[9,\"a\",3,\"b\",1,\"c\"]\n",
            "subset-invariants:ok\n",
            "single-invariant:ok\n",
            "weak-int:ok\n",
            "ValueError:array_rand(): Argument #1 ($array) must not be empty\n",
            "ValueError:array_rand(): Argument #1 ($array) must not be empty\n",
            "ValueError:array_rand(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)\n",
            "ValueError:array_rand(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)\n",
        )
    );
}

#[test]
fn array_key_value_functions_match_php_85_type_errors_in_strict_calls() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);

foreach (['array_keys', 'array_values', 'array_flip', 'array_count_values', 'array_rand'] as $name) {
    try {
        match ($name) {
            'array_keys' => array_keys('invalid'),
            'array_values' => array_values(null),
            'array_flip' => array_flip(1),
            'array_count_values' => array_count_values(false),
            'array_rand' => array_rand('invalid'),
        };
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}

try {
    array_keys([1], 1, 1);
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
try {
    array_rand([1], '1');
} catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "TypeError:array_keys(): Argument #1 ($array) must be of type array, string given\n",
            "TypeError:array_values(): Argument #1 ($array) must be of type array, null given\n",
            "TypeError:array_flip(): Argument #1 ($array) must be of type array, int given\n",
            "TypeError:array_count_values(): Argument #1 ($array) must be of type array, false given\n",
            "TypeError:array_rand(): Argument #1 ($array) must be of type array, string given\n",
            "TypeError:array_keys(): Argument #3 ($strict) must be of type bool, int given\n",
            "TypeError:array_rand(): Argument #2 ($num) must be of type int, string given\n",
        )
    );
}
