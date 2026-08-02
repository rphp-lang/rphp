<?php

class TypedMethodReturnSource
{
    function value(int $value): int
    {
        if (($value & 1) === 0) {
            return $value + 3;
        }
        return $value - 2;
    }
}

function typedMethodReturnConsume(TypedMethodReturnSource $source, int $value): int
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

function typedMethodReturnBenchmark(): int
{
    $source = new TypedMethodReturnSource();
    $sum = 0;
    for ($i = 0; $i < 2000000; $i++) {
        $sum = $sum + typedMethodReturnConsume($source, $i);
    }
    return $sum;
}

$start = microtime(true);
$result = typedMethodReturnBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
