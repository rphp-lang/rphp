<?php

function genericStep(int $value): int
{
    return $value + 1;
}

$start = microtime(true);
$value = 0;
for ($i = 0; $i < 5000000; $i++) {
    $value = genericStep($value);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
