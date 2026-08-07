<?php

function callback_walk_by_ref(&$value, $key)
{
    $value += $key & 1;
}

$count = 100000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$walked = array_walk($values, "callback_walk_by_ref");
$elapsed = microtime(true) - $startedAt;

$checksum = ($walked ? 1 : 0)
    + count($values)
    + $values[0]
    + $values[1]
    + $values[$count - 1];
echo $checksum . "|" . $elapsed;
