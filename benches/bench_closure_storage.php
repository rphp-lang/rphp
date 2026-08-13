<?php
// Retain many copies of one closure to measure shared payload footprint.
function run_closure_storage_benchmark(int $copies): string
{
    $prefix = 'kept';
    $offset = 7;
    $callback = static function (int $value) use ($prefix, $offset): int {
        return strlen($prefix) + $offset + $value;
    };
    $handlers = [];
    for ($index = 0; $index < $copies; ++$index) {
        $handlers[] = $callback;
    }
    $checksum = $handlers[0](1) + $handlers[$copies - 1](2);
    return count($handlers) . ',' . $checksum;
}

$start = microtime(true);
$result = run_closure_storage_benchmark(250000);
$elapsed = microtime(true) - $start;
echo $result, '|', $elapsed, "\n";
