<?php

function genericVariadicSum<T>(T ...$values): T
{
    return $values[0] + $values[1] + $values[2];
}

$start = microtime(true);
$value = 0;
for ($i = 0; $i < 1000000; $i++) {
    $value = genericVariadicSum::<int>($i, 1, 2);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
