<?php

function untypedFanoutSource($value)
{
    if (($value & 1) === 0) {
        return $value + 3;
    }
    return $value - 2;
}

function untypedFanoutConsume($value)
{
    $source = untypedFanoutSource($value);
    return ($source % 97)
        + ($source % 89)
        + ($source % 83)
        + ($source % 79)
        + ($source % 73)
        + ($source % 71)
        + ($source % 67)
        + ($source % 61);
}

function untypedIntFanoutBenchmark()
{
    $sum = 0;
    for ($i = 0; $i < 2000000; $i++) {
        $sum = $sum + untypedFanoutConsume($i);
    }
    return $sum;
}

$start = microtime(true);
$result = untypedIntFanoutBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
