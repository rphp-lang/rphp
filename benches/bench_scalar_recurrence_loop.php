<?php
// Two independent loop-carried Long values: induction and invariant deltas.
$n = 10000000;
$sum = 10;
$count = -5;
$step = 2;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum = $sum + $i;
    $count = $count + $step;
}
$elapsed = microtime(true) - $t;
echo $sum . ',' . $count . '|' . $elapsed;
