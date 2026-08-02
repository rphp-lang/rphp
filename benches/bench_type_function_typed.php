<?php

function typedStep(int $value, int $delta): int
{
    return (($value * 3) + $delta) % 1000003;
}

$start = microtime(true);
$value = 7;
for ($i = 0; $i < 5000000; $i++) {
    $value = typedStep($value, $i);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
