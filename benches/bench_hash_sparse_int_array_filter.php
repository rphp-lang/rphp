<?php
// Irregular integer-key reads with a data-dependent filtered aggregate.
$start = 1000000;
$n = 1000000;
$stride = 7;
$cutoff = 500000;
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
    $value = $values[$key];
    if ($value < $cutoff) {
        $sum += $value;
    }
    $key = $key + $stride;
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
