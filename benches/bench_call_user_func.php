<?php

function callback_step($value)
{
    return $value + 1;
}

$count = 5000000;
$sum = 0;
$startedAt = microtime(true);
for ($index = 0; $index < $count; $index++) {
    $sum += call_user_func('callback_step', $index);
}
$elapsed = microtime(true) - $startedAt;

echo $sum . '|' . $elapsed;
