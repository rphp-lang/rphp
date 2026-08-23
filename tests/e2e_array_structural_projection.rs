mod common;

use common::run_php;

#[test]
fn structural_array_projections_match_php_key_and_boundary_policies() {
    assert_eq!(
        run_php(
            r#"<?php
$input = [5 => 'a', 's' => 'b', 9 => 'c', 't' => 'd'];
echo json_encode(array_chunk($input, 2)), "\n";
echo json_encode(array_chunk($input, 2, true)), "\n";
echo json_encode(array_slice($input, 1, 2)), "\n";
echo json_encode(array_slice($input, 1, 2, true)), "\n";
echo json_encode(array_slice($input, -3, -1)), "\n";
echo json_encode(array_slice($input, PHP_INT_MIN, PHP_INT_MAX, true)), "\n";
echo json_encode(array_reverse($input)), "\n";
echo json_encode(array_reverse($input, true)), "\n";
echo json_encode(array_pad($input, 4, 'x')), "\n";
echo json_encode(array_pad($input, 6, 'x')), "\n";
echo json_encode(array_pad($input, -6, 'x')), "\n";

$large = range(0, 4096);
$chunks = array_chunk($large, 127);
$middle = array_slice($large, 2000, 5);
$reversed = array_reverse($large);
$padded = array_pad($middle, -8, -1);
echo count($chunks), ':', count($chunks[32]), ':', $chunks[32][32], '|';
echo implode(',', $middle), '|', $reversed[0], ':', $reversed[4096], '|';
echo implode(',', $padded);
"#,
        ),
        concat!(
            "[[\"a\",\"b\"],[\"c\",\"d\"]]\n",
            "[{\"5\":\"a\",\"s\":\"b\"},{\"9\":\"c\",\"t\":\"d\"}]\n",
            "{\"s\":\"b\",\"0\":\"c\"}\n",
            "{\"s\":\"b\",\"9\":\"c\"}\n",
            "{\"s\":\"b\",\"0\":\"c\"}\n",
            "{\"5\":\"a\",\"s\":\"b\",\"9\":\"c\",\"t\":\"d\"}\n",
            "{\"t\":\"d\",\"0\":\"c\",\"s\":\"b\",\"1\":\"a\"}\n",
            "{\"t\":\"d\",\"9\":\"c\",\"s\":\"b\",\"5\":\"a\"}\n",
            "{\"5\":\"a\",\"s\":\"b\",\"9\":\"c\",\"t\":\"d\"}\n",
            "{\"0\":\"a\",\"s\":\"b\",\"1\":\"c\",\"t\":\"d\",\"2\":\"x\",\"3\":\"x\"}\n",
            "{\"0\":\"x\",\"1\":\"x\",\"2\":\"a\",\"s\":\"b\",\"3\":\"c\",\"t\":\"d\"}\n",
            "33:33:4096|2000,2001,2002,2003,2004|4096:0|",
            "-1,-1,-1,2000,2001,2002,2003,2004",
        )
    );
}

#[test]
fn structural_array_projections_preserve_source_references_and_cow_values() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['chunk', 'slice', 'reverse', 'pad'] as $which) {
    $slot = 10;
    $source = [5 => &$slot, 'nested' => ['v' => 20], 9 => 30];
    $result = match ($which) {
        'chunk' => array_chunk($source, 3, true)[0],
        'slice' => array_slice($source, 0, null, true),
        'reverse' => array_reverse($source, true),
        'pad' => array_pad($source, 4, 0),
    };
    $key = $which === 'pad' ? 0 : 5;
    $result[$key]++;
    $result['nested']['v'] = 99;
    echo "$which:$slot:", $source['nested']['v'], "\n";
}

foreach (['chunk', 'slice', 'reverse', 'pad'] as $which) {
    $slot = 10;
    $source = [&$slot, 20];
    $result = match ($which) {
        'chunk' => array_chunk($source, 2)[0],
        'slice' => array_slice($source, 0),
        'reverse' => array_reverse($source),
        'pad' => array_pad($source, 3, 0),
    };
    $key = $which === 'reverse' ? 1 : 0;
    $result[$key]++;
    echo "packed-$which:$slot\n";
}

$padding = 40;
$padding_alias = &$padding;
$result = array_pad([1], 3, $padding_alias);
$result[1] = 99;
echo "pad-value:$padding:", $result[2], "\n";

$source = [5 => 'a', 9 => 'b'];
$preserve = false;
$preserve_alias = &$preserve;
set_error_handler(function () use (&$source, &$preserve) {
    $source[11] = 'late';
    $preserve = true;
    return true;
});
$slice = array_slice($source, 0.5, null, $preserve_alias);
restore_error_handler();
echo json_encode($source), '|', json_encode($slice), "\n";

$padding = 'before';
$padding_alias = &$padding;
set_error_handler(function () use (&$padding) {
    $padding = 'after';
    return true;
});
$result = array_pad([1], 2.5, $padding_alias);
restore_error_handler();
echo $padding, '|', json_encode($result);
"#,
        ),
        concat!(
            "chunk:11:20\n",
            "slice:11:20\n",
            "reverse:11:20\n",
            "pad:11:20\n",
            "packed-chunk:11\n",
            "packed-slice:11\n",
            "packed-reverse:11\n",
            "packed-pad:11\n",
            "pad-value:40:40\n",
            "{\"5\":\"a\",\"9\":\"b\",\"11\":\"late\"}|[\"a\",\"b\"]\n",
            "after|[1,\"before\"]",
        )
    );
}

#[test]
fn structural_array_projection_diagnostics_and_coercions_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; return true; });
echo json_encode(array_chunk([1, 2, 3], "2.9")), "\n";
echo json_encode(array_slice(['a', 'b', 'c'], "1.9", "1.2", 1)), "\n";
echo json_encode(array_reverse([5 => 'a'], 1)), "\n";
echo json_encode(array_pad(['a'], "3.7", 'x')), "\n";
restore_error_handler();

foreach ([
    fn() => array_chunk(true, 1),
    fn() => array_slice([], 0, 'bad'),
    fn() => array_reverse([], []),
    fn() => array_pad([], [], 0),
    fn() => array_chunk([], 0),
    fn() => array_pad([], PHP_INT_MAX, null),
    fn() => array_pad([], PHP_INT_MIN, null),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}

set_error_handler(function ($level, $message) { throw new Exception("handled:$message"); });
try { array_chunk([1], null); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
restore_error_handler();

eval(<<<'PHP'
declare(strict_types=1);
foreach ([
    fn() => array_chunk([], '2'),
    fn() => array_slice([], true),
    fn() => array_reverse([], 1),
    fn() => array_pad([], 2.0, 0),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
PHP);
"#,
        ),
        concat!(
            "8192:Implicit conversion from float-string \"2.9\" to int loses precision\n",
            "[[1,2],[3]]\n",
            "8192:Implicit conversion from float-string \"1.9\" to int loses precision\n",
            "8192:Implicit conversion from float-string \"1.2\" to int loses precision\n",
            "{\"1\":\"b\"}\n",
            "{\"5\":\"a\"}\n",
            "8192:Implicit conversion from float-string \"3.7\" to int loses precision\n",
            "[\"a\",\"x\",\"x\"]\n",
            "TypeError:array_chunk(): Argument #1 ($array) must be of type array, true given\n",
            "TypeError:array_slice(): Argument #3 ($length) must be of type ?int, string given\n",
            "TypeError:array_reverse(): Argument #2 ($preserve_keys) must be of type bool, array given\n",
            "TypeError:array_pad(): Argument #2 ($length) must be of type int, array given\n",
            "ValueError:array_chunk(): Argument #2 ($length) must be greater than 0\n",
            "ValueError:array_pad(): Argument #2 ($length) must not exceed the maximum allowed array size\n",
            "ValueError:array_pad(): Argument #2 ($length) must not exceed the maximum allowed array size\n",
            "Exception:handled:array_chunk(): Passing null to parameter #2 ($length) of type int is deprecated\n",
            "TypeError:array_chunk(): Argument #2 ($length) must be of type int, string given\n",
            "TypeError:array_slice(): Argument #2 ($offset) must be of type int, true given\n",
            "TypeError:array_reverse(): Argument #2 ($preserve_keys) must be of type bool, int given\n",
            "TypeError:array_pad(): Argument #2 ($length) must be of type int, float given\n",
        )
    );
}

#[test]
fn structural_array_projection_signatures_and_named_arguments_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['array_chunk', 'array_slice', 'array_reverse', 'array_pad'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), '/', $reflection->getNumberOfParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), ',', $parameter->isOptional() ? 'optional' : 'required', ';';
    }
    echo "\n";
}
$input = [5 => 'a', 's' => 'b', 9 => 'c'];
echo json_encode(array_chunk(array: $input, length: 2, preserve_keys: true)), "\n";
echo json_encode(array_slice(array: $input, offset: 1, preserve_keys: true)), "\n";
echo json_encode(array_reverse(array: $input, preserve_keys: true)), "\n";
echo json_encode(array_pad(array: $input, length: 5, value: 'x'));
"#,
        ),
        concat!(
            "array_chunk:2/3:array,required;length,required;preserve_keys,optional;\n",
            "array_slice:2/4:array,required;offset,required;length,optional;preserve_keys,optional;\n",
            "array_reverse:1/2:array,required;preserve_keys,optional;\n",
            "array_pad:3/3:array,required;length,required;value,required;\n",
            "[{\"5\":\"a\",\"s\":\"b\"},{\"9\":\"c\"}]\n",
            "{\"s\":\"b\",\"9\":\"c\"}\n",
            "{\"9\":\"c\",\"s\":\"b\",\"5\":\"a\"}\n",
            "{\"0\":\"a\",\"s\":\"b\",\"1\":\"c\",\"2\":\"x\",\"3\":\"x\"}",
        )
    );
}
