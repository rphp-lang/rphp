<?php

$count = 500000;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = $index;
}

$startedAt = microtime(true);
$mapped = array_map(null, $values);
$elapsed = microtime(true) - $startedAt;

echo $mapped[0] . ":" . $mapped[$count - 1] . ":" . count($mapped) . "|" . $elapsed;
