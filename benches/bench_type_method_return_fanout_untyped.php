<?php

class UntypedMethodReturnSource
{
    function value(int $value)
    {
        if (($value & 1) === 0) {
            return $value + 3;
        }
        return $value - 2;
    }
}

function untypedMethodReturnConsume(UntypedMethodReturnSource $source, int $value): int
{
    $result = $source->value($value);
    return ($result % 97)
        + ($result % 89)
        + ($result % 83)
        + ($result % 79)
        + ($result % 73)
        + ($result % 71)
        + ($result % 67)
        + ($result % 61);
}

function untypedMethodReturnBenchmark(): int
{
    $source = new UntypedMethodReturnSource();
    $sum = 0;
    for ($i = 0; $i < 2000000; $i++) {
        $sum = $sum + untypedMethodReturnConsume($source, $i);
    }
    return $sum;
}

$start = microtime(true);
$result = untypedMethodReturnBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
