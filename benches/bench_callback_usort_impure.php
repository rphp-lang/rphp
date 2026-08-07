<?php

$comparisons = 0;
function callback_usort_impure($left, $right)
{
    global $comparisons;
    $comparisons++;
    return $left - $right;
}

$count = 500;
$values = [];
for ($index = $count - 1; $index >= 0; $index--) {
    $values[] = $index;
}

$startedAt = microtime(true);
$sorted = usort($values, "callback_usort_impure");
$elapsed = microtime(true) - $startedAt;

$checksum = ($sorted ? 1 : 0)
    + count($values)
    + $values[0]
    + $values[$count - 1]
    + $comparisons;
echo $checksum . "|" . $elapsed;
