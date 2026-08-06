<?php
// Read control for the fully materialized irregular integer index.
$n = 1000000;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
