<?php

function typedCalculate($value)
{
    return ($value * 2) + 1;
}

$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 10000000; $i++) {
    $sum = $sum + typedCalculate($i);
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
