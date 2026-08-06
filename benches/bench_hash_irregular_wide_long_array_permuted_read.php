<?php
// Control for the canonical position-based fallback: values exceed the compact
// Long payload range while keys retain the same permuted read order.
$n = 1048576;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i + 1099511627776;
}
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $position = ($i * 48271) & 1048575;
    $key = (($position * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
