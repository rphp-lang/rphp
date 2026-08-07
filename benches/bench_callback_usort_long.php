<?php

function callback_usort_long($left, $right)
{
    return $left - $right;
}

$count = 4096;
$values = [];
for ($index = 0; $index < $count; $index++) {
    $values[] = ($index * 48271) & ($count - 1);
}

$startedAt = microtime(true);
$sorted = usort($values, "callback_usort_long");
$elapsed = microtime(true) - $startedAt;

$checksum = ($sorted ? 1 : 0)
    + count($values)
    + $values[0]
    + $values[$count - 1];
echo $checksum . "|" . $elapsed;
