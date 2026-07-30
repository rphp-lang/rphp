<?php
// Common parity branch: typed modulo, equality and conditional accumulation.
$n = 10000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    if (($i % 2) == 0) {
        $sum += $i;
    }
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
