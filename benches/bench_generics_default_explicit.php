<?php

function genericDefault<T>(T $value = 1): T
{
    return $value;
}

$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 2000000; $i++) {
    $sum += genericDefault::<int>(1);
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
