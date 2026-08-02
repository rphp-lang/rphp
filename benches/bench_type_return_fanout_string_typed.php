<?php

function typedFanoutLabel(int $value): string
{
    if (($value & 1) === 0) {
        return 'typed-even';
    }
    return 'typed-odd';
}

function typedStringFanoutConsume(int $value): int
{
    $label = typedFanoutLabel($value);
    return strlen($label) + strlen($label) + strlen($label) + strlen($label)
        + strlen($label) + strlen($label) + strlen($label) + strlen($label);
}

function typedStringFanoutBenchmark(): int
{
    $sum = 0;
    for ($i = 0; $i < 1000000; $i++) {
        $sum = $sum + typedStringFanoutConsume($i);
    }
    return $sum;
}

$start = microtime(true);
$result = typedStringFanoutBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
