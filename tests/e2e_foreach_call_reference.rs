mod common;

use common::run_php;

#[test]
fn foreach_reference_call_results_distinguish_value_and_reference_returns() {
    assert_eq!(
        run_php(
            r#"<?php
function returned_value($array) {
    echo "value-call\n";
    return $array;
}
function &returned_reference(&$array) {
    echo "reference-call\n";
    return $array;
}

$source = [1, 2, 3];
$copy = $source;
foreach (returned_value($source) as $key => &$value) {
    echo "V:$key:$value\n";
    $value *= 10;
    if ($key === 1) { break; }
}
$value = 77;
echo 'value-source=', implode(',', $source), "\n";
echo 'value-copy=', implode(',', $copy), "\n";
unset($value);

foreach (returned_reference($source) as $key => &$value) {
    echo "R:$key:$value\n";
    $value += 5;
}
$value = 99;
echo 'reference-source=', implode(',', $source), "\n";
echo 'reference-copy=', implode(',', $copy), "\n";
unset($value);
"#,
        ),
        concat!(
            "value-call\n",
            "V:0:1\n",
            "V:1:2\n",
            "value-source=1,2,3\n",
            "value-copy=1,2,3\n",
            "reference-call\n",
            "R:0:1\n",
            "R:1:2\n",
            "R:2:3\n",
            "reference-source=6,7,99\n",
            "reference-copy=1,2,3\n",
        )
    );
}

#[test]
fn foreach_reference_call_results_cover_method_static_and_dynamic_calls() {
    assert_eq!(
        run_php(
            r#"<?php
class Sources {
    public $instance = [1, 2];
    public static $shared = [3, 4];
    public function instanceValue() { return $this->instance; }
    public function &instanceReference() { return $this->instance; }
    public static function staticValue() { return self::$shared; }
    public static function &staticReference() { return self::$shared; }
}

$source = new Sources();
foreach ($source->instanceValue() as &$value) { $value *= 10; }
$value = 77;
unset($value);
echo 'instance-value=', implode(',', $source->instance), "\n";

foreach ($source->instanceReference() as &$value) { $value += 5; }
$value = 99;
unset($value);
echo 'instance-reference=', implode(',', $source->instance), "\n";

foreach (Sources::staticValue() as &$value) { $value *= 10; }
unset($value);
echo 'static-value=', implode(',', Sources::$shared), "\n";

foreach (Sources::staticReference() as &$value) { $value += 6; }
$value = 88;
unset($value);
echo 'static-reference=', implode(',', Sources::$shared), "\n";

$class = Sources::class;
$method = 'staticReference';
foreach ($class::$method() as &$value) { ++$value; break; }
unset($value);
echo 'dynamic-static-reference=', implode(',', Sources::$shared), "\n";

$dynamic = [$source, 'instanceReference'];
foreach ($dynamic() as &$value) { ++$value; break; }
unset($value);
echo 'dynamic-reference=', implode(',', $source->instance), "\n";
"#,
        ),
        concat!(
            "instance-value=1,2\n",
            "instance-reference=6,99\n",
            "static-value=3,4\n",
            "static-reference=9,88\n",
            "dynamic-static-reference=10,88\n",
            "dynamic-reference=7,99\n",
        )
    );
}

#[test]
fn foreach_reference_call_results_preserve_interior_aliases_and_unwind_state() {
    assert_eq!(
        run_php(
            r#"<?php
function returned_value($array) { return $array; }
function &returned_reference(&$array) { return $array; }

$shared = 4;
$source = [&$shared, 5];
$copy = $source;
foreach (returned_value($source) as &$value) { $value += 10; }
unset($value);
echo 'interior=', $shared, '|', implode(',', $source), '|', implode(',', $copy), "\n";

$calls = 0;
function &counted_reference(&$array) {
    global $calls;
    ++$calls;
    return $array;
}
foreach (counted_reference($source) as $outer_key => &$outer) {
    foreach (counted_reference($source) as $inner_key => &$inner) {
        if ($outer_key === 0 && $inner_key === 0) { $inner += 1; }
        break;
    }
    unset($inner);
    $outer += 2;
    break;
}
unset($outer);
echo 'nested=', $calls, '|', $shared, '|', implode(',', $source), "\n";

try {
    foreach (returned_reference($source) as $key => &$value) {
        $value *= 2;
        if ($key === 1) { throw new Exception('stop'); }
    }
} catch (Exception $exception) {
    echo 'caught=', $exception->getMessage(), "\n";
}
$value = 123;
echo 'exception=', $shared, '|', implode(',', $source), "\n";
unset($value);
"#,
        ),
        concat!(
            "interior=14|14,5|14,5\n",
            "nested=2|17|17,5\n",
            "caught=stop\n",
            "exception=34|34,123\n",
        )
    );
}

#[test]
fn foreach_reference_call_result_reports_runtime_iterable_warnings_once() {
    assert_eq!(
        run_php(
            r#"<?php
$calls = 0;
function scalar_source() {
    global $calls;
    ++$calls;
    echo "scalar-call\n";
    return 42;
}
set_error_handler(function ($severity, $message) {
    echo "handler:$severity:$message\n";
    return true;
});
foreach (scalar_source() as &$value) { echo "body\n"; }
restore_error_handler();
echo "after:$calls\n";
"#,
        ),
        concat!(
            "scalar-call\n",
            "handler:2:foreach() argument must be of type array|object, int given\n",
            "after:1\n",
        )
    );
}

#[test]
fn foreach_reference_call_results_keep_iterator_and_generator_errors_catchable() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainIterator implements Iterator {
    public function current(): mixed { return 1; }
    public function key(): mixed { return 0; }
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool { return false; }
}
function iterator_source(): Iterator { return new PlainIterator(); }
function generator_source(): Generator { yield 1; }

foreach ([
    'iterator' => fn() => iterator_source(),
    'generator' => fn() => generator_source(),
] as $name => $factory) {
    try {
        foreach ($factory() as &$value) { echo "body:$name\n"; }
    } catch (Throwable $throwable) {
        echo $name, ':', get_class($throwable), ':', $throwable->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "iterator:Error:An iterator cannot be used with foreach by reference\n",
            "generator:Exception:You can only iterate a generator by-reference ",
            "if it declared that it yields by-reference\n",
        )
    );
}
