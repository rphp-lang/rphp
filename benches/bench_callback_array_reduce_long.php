<?php

function callback_sum_long($carry, $value)
{
    return $carry + $value;
}

$count = 500000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$sum = array_reduce($values, "callback_sum_long", 0);
$elapsed = microtime(true) - $startedAt;

echo $sum . "|" . $elapsed;
