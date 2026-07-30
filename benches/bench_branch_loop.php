<?php
// Scalar loop with a branch that changes direction halfway through.
$n = 10000000;
$cutoff = 5000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $sum += $i;
    }
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
