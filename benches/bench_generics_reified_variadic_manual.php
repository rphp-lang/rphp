<?php

function manualVariadicSum(int ...$values): int
{
    return $values[0] + $values[1] + $values[2];
}

$start = microtime(true);
$value = 0;
for ($i = 0; $i < 1000000; $i++) {
    $value = manualVariadicSum($i, 1, 2);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
