<?php
// Repeated dynamic dispatch through PHP's [Closure, "__invoke"] callback form.
function run_closure_invoke_array_benchmark(int $iterations): int
{
    $offset = 7;
    $closure = static function (int $value) use ($offset): int {
        return $value + $offset;
    };
    $callback = [$closure, '__invoke'];
    $sum = 0;
    for ($index = 0; $index < $iterations; ++$index) {
        $sum += $callback($index & 255);
    }
    return $sum;
}

$start = microtime(true);
$result = run_closure_invoke_array_benchmark(250000);
$elapsed = microtime(true) - $start;
echo $result, '|', $elapsed, "\n";
