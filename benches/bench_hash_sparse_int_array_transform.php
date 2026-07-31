<?php
// Irregular integer-key reads with a materialized value and two aggregates.
$start = 1000000;
$n = 1000000;
$stride = 7;
$values = [];
$key = $start;
for ($i = 0; $i < $n; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$sum = 0;
$adjusted = 0;
$one = 1;
$key = $start;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $value = $values[$key];
    $sum += $value;
    $adjusted += $value + $one;
    $key = $key + $stride;
}
$elapsed = microtime(true) - $t;
echo $sum . ':' . $adjusted . '|' . $elapsed;
