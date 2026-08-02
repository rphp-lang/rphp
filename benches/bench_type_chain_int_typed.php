<?php

function typedSource(int $value): int
{
    if (($value & 1) === 0) {
        return $value + 3;
    }
    return $value - 2;
}

function typedConsume(int $value): int
{
    return (typedSource($value) % 97) ^ 13;
}

function typedChainBenchmark(): int
{
    $sum = 0;
    for ($i = 0; $i < 5000000; $i++) {
        $sum = $sum + typedConsume($i);
    }
    return $sum;
}

$start = microtime(true);
$result = typedChainBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
