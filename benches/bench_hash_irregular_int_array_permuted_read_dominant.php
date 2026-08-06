<?php
// Read-dominant profiling holdout for the canonical irregular integer index.
// Repeating the same full permutation keeps construction outside the measured
// interval while giving sampling profilers enough indexed-lookup work.
$n = 1048576;
$passes = 8;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$sum = 0;
$t = microtime(true);
for ($pass = 0; $pass < $passes; $pass++) {
    for ($i = 0; $i < $n; $i++) {
        $position = ($i * 48271) & 1048575;
        $key = (($position * 1103515245) & 2147483647) + 1000000;
        $sum += $values[$key];
    }
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
