<?php

function callback_keep_odd($value)
{
    return $value & 1;
}

$count = 500000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$filtered = array_filter($values, "callback_keep_odd");
$elapsed = microtime(true) - $startedAt;

echo count($filtered) . ":" . $filtered[1] . ":" . $filtered[$count - 1] . "|" . $elapsed;
