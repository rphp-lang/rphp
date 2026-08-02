<?php

function untypedLabel($value)
{
    if (($value & 1) === 0) {
        return 'even';
    }
    return 'odd';
}

function untypedLabelLength($value)
{
    return strlen(untypedLabel($value) . '!');
}

function untypedStringChainBenchmark()
{
    $sum = 0;
    for ($i = 0; $i < 1000000; $i++) {
        $sum = $sum + untypedLabelLength($i);
    }
    return $sum;
}

$start = microtime(true);
$result = untypedStringChainBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
