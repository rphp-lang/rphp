mod common;

use common::run_php;

#[test]
fn range_matches_integer_float_numeric_string_and_byte_modes() {
    assert_eq!(
        run_php(
            r#"<?php
function show_range_case(string $label, array $arguments): void {
    echo $label, ':';
    foreach (range(...$arguments) as $value) {
        echo gettype($value)[0], '=', $value, ';';
    }
    echo "\n";
}

show_range_case('integer-up', [-2, 4, 2]);
show_range_case('integer-down', [7, 1, 3]);
show_range_case('integral-float-step', [1, 5, 2.0]);
show_range_case('float-down', [4.5, 4.2, 0.1]);
show_range_case('numeric-text', ['10', '13']);
show_range_case('digit-bytes', ['7', '9']);
show_range_case('character-down', ['f', 'a', -2]);
show_range_case('int-min', [PHP_INT_MIN + 3, PHP_INT_MIN]);
"#,
        ),
        concat!(
            "integer-up:i=-2;i=0;i=2;i=4;\n",
            "integer-down:i=7;i=4;i=1;\n",
            "integral-float-step:i=1;i=3;i=5;\n",
            "float-down:d=4.5;d=4.4;d=4.3;d=4.2;\n",
            "numeric-text:i=10;i=11;i=12;i=13;\n",
            "digit-bytes:s=7;s=8;s=9;\n",
            "character-down:s=f;s=d;s=b;\n",
            "int-min:i=-9223372036854775805;i=-9223372036854775806;",
            "i=-9223372036854775807;i=-9223372036854775808;\n",
        )
    );
}

#[test]
fn range_matches_php_85_diagnostics_and_safe_size_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
function show_range_case(string $label, array $arguments): void {
    echo $label, ':';
    try {
        foreach (range(...$arguments) as $value) {
            echo gettype($value)[0], '=', $value, ';';
        }
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage();
    }
    echo "\n";
}

set_error_handler(function (int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
show_range_case('wide-bytes', ['AA', 'DD', 2]);
show_range_case('numeric-character', ['3.5', 'A']);
show_range_case('fractional-character', ['A', 'D', 0.5]);
show_range_case('empty-character', ['', 'C']);
restore_error_handler();

foreach ([
    [0, PHP_INT_MAX],
    [fdiv(0, 0), 1],
    [1, INF],
    [1, 3, -1],
    [1, 3, 3],
] as $arguments) {
    show_range_case('error', $arguments);
}
"#,
        ),
        concat!(
            "wide-bytes:2:range(): Argument #1 ($start) must be a single byte, subsequent bytes are ignored\n",
            "2:range(): Argument #2 ($end) must be a single byte, subsequent bytes are ignored\n",
            "s=A;s=C;\n",
            "numeric-character:2:range(): Argument #1 ($start) must be a single byte string if argument #2 ($end) is a single byte string, argument #2 ($end) converted to 0\n",
            "d=3.5;d=2.5;d=1.5;d=0.5;\n",
            "fractional-character:2:range(): Argument #3 ($step) must be of type int when generating an array of characters, inputs converted to 0\n",
            "d=0;\n",
            "empty-character:2:range(): Argument #1 ($start) must not be empty, casted to 0\n",
            "2:range(): Argument #1 ($start) must be a single byte string if argument #2 ($end) is a single byte string, argument #2 ($end) converted to 0\n",
            "i=0;\n",
            "error:ValueError:The supplied range exceeds the maximum array size by ",
            "9223372035781033984 elements: start=0, end=9223372036854775807, ",
            "step=1. Calculated size: 9223372036854775807. Maximum size: 1073741824.\n",
            "error:ValueError:range(): Argument #1 ($start) must be a finite number, NAN provided\n",
            "error:ValueError:range(): Argument #2 ($end) must be a finite number, INF provided\n",
            "error:ValueError:range(): Argument #3 ($step) must be greater than 0 for increasing ranges\n",
            "error:ValueError:range(): Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)\n",
        )
    );
}

#[test]
fn array_fill_and_combine_preserve_keys_references_cow_and_snapshot_order() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
$combined = array_combine(
    [true, false, null, 1.5, '2', '02', [1]],
    ['t', 'f', 'n', 'd', 'two', 'zero-two', 'array'],
);
restore_error_handler();
echo 'combined:';
foreach ($combined as $key => $value) {
    echo is_int($key) ? 'i=' : 's=', $key, '>', $value, ';';
}
echo "\n";

$slot = 10;
$values = [&$slot, ['nested' => 1]];
$combined = array_combine(['alias', 'copy'], $values);
$slot = 20;
$combined['copy']['nested'] = 2;
echo 'combine-cow:', $combined['alias'], ':', $values[1]['nested'], "\n";
$combined['alias'] = 30;
echo 'combine-reference:', $slot, ':', $values[0], "\n";

class ReentrantConstructionKey {
    public function __toString(): string {
        global $constructorKeys, $constructorValues;
        $constructorKeys[] = 'late';
        $constructorValues[] = 99;
        return 'first';
    }
}
$constructorKeys = [new ReentrantConstructionKey(), 'second'];
$constructorValues = [1, 2];
echo 'combine-snapshot:', json_encode(array_combine($constructorKeys, $constructorValues)), ':';
echo count($constructorKeys), ':', count($constructorValues), "\n";

$slot = 40;
$slotAlias = &$slot;
$filled = array_fill(-2, 3, $slotAlias);
$slot = 41;
$filled[-2] = 50;
echo 'fill-reference:', $slot, ':', json_encode($filled), "\n";
$nested = ['v' => 1];
$filled = array_fill(0, 2, $nested);
$filled[0]['v'] = 2;
echo 'fill-cow:', $nested['v'], ':', $filled[1]['v'], "\n";
$object = (object) ['v' => 1];
$filled = array_fill(0, 2, $object);
$filled[0]->v = 2;
echo 'fill-object:', $filled[1]->v, "\n";

foreach ([
    fn() => array_combine([], [1]),
    fn() => array_combine(null, []),
    fn() => array_fill(0, -1, null),
    fn() => array_fill(0, 2147483648, null),
    fn() => array_fill(PHP_INT_MAX - 1, 3, null),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
$edge = array_fill(PHP_INT_MAX, 1, 'edge');
try { $edge[] = 'overflow'; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
$mergeInput = range(0, 262143);
try { array_merge(...array_fill(0, 4096, $mergeInput)); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "2:Array to string conversion\n",
            "combined:i=1>t;s=>n;s=1.5>d;i=2>two;s=02>zero-two;s=Array>array;\n",
            "combine-cow:20:1\n",
            "combine-reference:30:30\n",
            "combine-snapshot:{\"first\":1,\"second\":2}:3:3\n",
            "fill-reference:41:{\"-2\":50,\"-1\":40,\"0\":40}\n",
            "fill-cow:1:1\n",
            "fill-object:2\n",
            "ValueError:array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements\n",
            "TypeError:array_combine(): Argument #1 ($keys) must be of type array, null given\n",
            "ValueError:array_fill(): Argument #2 ($count) must be greater than or equal to 0\n",
            "ValueError:array_fill(): Argument #2 ($count) is too large\n",
            "Error:Cannot add element to the array as the next element is already occupied\n",
            "Error:Cannot add element to the array as the next element is already occupied\n",
            "Error:The total number of elements must be lower than 1073741824\n",
        )
    );
}

#[test]
fn array_constructor_signatures_conversions_named_arguments_and_handlers_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): never {
    throw new Exception('handled:' . $message);
});
try { range('', 'A'); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { array_combine([[1]], [2]); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
restore_error_handler();

foreach (['range', 'array_fill', 'array_combine'] as $name) {
    $reflection = new ReflectionFunction($name);
    echo $name, ':', $reflection->getNumberOfRequiredParameters(), '/', $reflection->getNumberOfParameters(), ':';
    foreach ($reflection->getParameters() as $parameter) {
        echo $parameter->getName(), ',', $parameter->isOptional() ? 'optional' : 'required', ';';
    }
    echo "\n";
}
echo json_encode(range(start: 2, end: 6, step: 2)), "\n";
echo json_encode(array_fill(start_index: -1, count: 2, value: 'x')), "\n";
echo json_encode(array_combine(keys: ['a', 'b'], values: [1, 2])), "\n";

function show_weak_range(): void {
    echo 'weak-null:';
    foreach (range(null, 2) as $value) { echo gettype($value)[0], '=', $value, ';'; }
    echo "\n";
}
set_error_handler(function (int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
echo json_encode(array_fill(1.9, '2.8', 'x')), "\n";
show_weak_range();
restore_error_handler();

eval(<<<'PHP'
declare(strict_types=1);
foreach ([
    fn() => range(true, 2),
    fn() => range(1, 2, '1'),
    fn() => array_fill(0.0, 1, null),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
PHP);
"#,
        ),
        concat!(
            "Exception:handled:range(): Argument #1 ($start) must not be empty, casted to 0\n",
            "Exception:handled:Array to string conversion\n",
            "range:2/3:start,required;end,required;step,optional;\n",
            "array_fill:3/3:start_index,required;count,required;value,required;\n",
            "array_combine:2/2:keys,required;values,required;\n",
            "[2,4,6]\n",
            "{\"-1\":\"x\",\"0\":\"x\"}\n",
            "{\"a\":1,\"b\":2}\n",
            "8192:Implicit conversion from float 1.9 to int loses precision\n",
            "8192:Implicit conversion from float-string \"2.8\" to int loses precision\n",
            "{\"1\":\"x\",\"2\":\"x\"}\n",
            "weak-null:8192:range(): Passing null to parameter #1 ($start) of type string|int|float is deprecated\n",
            "i=0;i=1;i=2;\n",
            "TypeError:range(): Argument #1 ($start) must be of type string|int|float, true given\n",
            "TypeError:range(): Argument #3 ($step) must be of type int|float, string given\n",
            "TypeError:array_fill(): Argument #1 ($start_index) must be of type int, float given\n",
        )
    );
}
