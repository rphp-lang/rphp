<?php
// Irregular integer-index control whose read order deliberately differs from
// insertion order. A speculative ordered cursor must disable itself quickly
// and retain the canonical indexed lookup cost.
$n = 1048576;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
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
