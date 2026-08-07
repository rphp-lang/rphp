<?php

function callback_walk_long($value, $key)
{
    return $value * 3 + $key;
}

$count = 500000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$walked = array_walk($values, "callback_walk_long");
$elapsed = microtime(true) - $startedAt;

$checksum = ($walked ? 1 : 0) + count($values) + $values[0] + $values[$count - 1];
echo $checksum . "|" . $elapsed;
