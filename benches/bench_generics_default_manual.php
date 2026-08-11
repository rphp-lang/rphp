<?php

function genericDefault(int $value = 1): int
{
    return $value;
}

$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 2000000; $i++) {
    $sum += genericDefault();
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
