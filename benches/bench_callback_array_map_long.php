<?php

function callback_map_long($value)
{
    return $value * 3 + 1;
}

$count = 500000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$mapped = array_map("callback_map_long", $values);
$elapsed = microtime(true) - $startedAt;

echo $mapped[0] . ":" . $mapped[$count - 1] . ":" . count($mapped) . "|" . $elapsed;
