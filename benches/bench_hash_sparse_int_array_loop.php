<?php
// Irregular integer-key reads that must use the hash index fallback.
$start = 1000000;
$n = 250000;
$stride = 7;
$values = [];
$key = $start;
for ($i = 0; $i < $n; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$sum = 0;
$key = $start;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum += $values[$key];
    $key = $key + $stride;
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
