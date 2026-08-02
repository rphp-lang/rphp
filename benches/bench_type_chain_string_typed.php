<?php

function typedLabel(int $value): string
{
    if (($value & 1) === 0) {
        return 'even';
    }
    return 'odd';
}

function typedLabelLength(int $value): int
{
    return strlen(typedLabel($value) . '!');
}

function typedStringChainBenchmark(): int
{
    $sum = 0;
    for ($i = 0; $i < 1000000; $i++) {
        $sum = $sum + typedLabelLength($i);
    }
    return $sum;
}

$start = microtime(true);
$result = typedStringChainBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
