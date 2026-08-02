<?php

function untypedSource($value)
{
    if (($value & 1) === 0) {
        return $value + 3;
    }
    return $value - 2;
}

function untypedConsume($value)
{
    return (untypedSource($value) % 97) ^ 13;
}

function untypedChainBenchmark()
{
    $sum = 0;
    for ($i = 0; $i < 5000000; $i++) {
        $sum = $sum + untypedConsume($i);
    }
    return $sum;
}

$start = microtime(true);
$result = untypedChainBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
