<?php
// Isolate copies of one immutable closure payload with a captured heap value.
function invoke_copied_closure(Closure $callback, int $value): int
{
    return $callback($value);
}

function run_closure_copy_benchmark(int $iterations): int
{
    $prefix = 'kept';
    $offset = 7;
    $callback = static function (int $value) use ($prefix, $offset): int {
        return strlen($prefix) + $offset + $value;
    };
    $sum = 0;
    for ($index = 0; $index < $iterations; ++$index) {
        $copy = $callback;
        $sum += invoke_copied_closure($copy, $index & 255);
    }
    return $sum;
}

$start = microtime(true);
$result = run_closure_copy_benchmark(250000);
$elapsed = microtime(true) - $start;
echo $result, '|', $elapsed, "\n";
