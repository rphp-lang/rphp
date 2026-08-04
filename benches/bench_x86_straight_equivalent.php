<?php
// Source equivalent of the first x86-64 straight-IR additive recurrence.
$n = 10000000;
$sum = 10;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum = $sum + $i;
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
