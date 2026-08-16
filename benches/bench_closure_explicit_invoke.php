<?php
// Repeated explicit dispatch through Closure::__invoke().
function run_closure_explicit_invoke_benchmark(int $iterations): int
{
    $offset = 7;
    $closure = static function (int $value) use ($offset): int {
        return $value + $offset;
    };
    $sum = 0;
    for ($index = 0; $index < $iterations; ++$index) {
        $sum += $closure->__invoke($index & 255);
    }
    return $sum;
}

$start = microtime(true);
$result = run_closure_explicit_invoke_benchmark(250000);
$elapsed = microtime(true) - $start;
echo $result, '|', $elapsed, "\n";
