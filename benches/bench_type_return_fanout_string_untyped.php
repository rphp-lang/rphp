<?php

function untypedFanoutLabel($value)
{
    if (($value & 1) === 0) {
        return 'typed-even';
    }
    return 'typed-odd';
}

function untypedStringFanoutConsume($value)
{
    $label = untypedFanoutLabel($value);
    return strlen($label) + strlen($label) + strlen($label) + strlen($label)
        + strlen($label) + strlen($label) + strlen($label) + strlen($label);
}

function untypedStringFanoutBenchmark()
{
    $sum = 0;
    for ($i = 0; $i < 1000000; $i++) {
        $sum = $sum + untypedStringFanoutConsume($i);
    }
    return $sum;
}

$start = microtime(true);
$result = untypedStringFanoutBenchmark();
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
