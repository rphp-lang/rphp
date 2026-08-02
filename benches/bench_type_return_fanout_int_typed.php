<?php

function typedFanoutSource(int $value): int
{
    if (($value & 1) === 0) {
        return $value + 3;
    }
    return $value - 2;
}

function typedFanoutConsume(int $value): int
{
    $source = typedFanoutSource($value);
    return ($source % 97)
        + ($source % 89)
        + ($source % 83)
        + ($source % 79)
        + ($source % 73)
        + ($source % 71)
        + ($source % 67)
        + ($source % 61);
}

function typedIntFanoutBenchmark(): int
{
    $sum = 0;
    for ($i = 0; $i < 2000000; $i++) {
        $sum = $sum + typedFanoutConsume($i);
    }
    return $sum;
}

$start = microtime(true);
$result = typedIntFanoutBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
