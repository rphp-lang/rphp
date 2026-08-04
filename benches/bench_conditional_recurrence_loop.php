<?php
// A guarded recurrence plus an unconditional recurrence in one scalar region.
$n = 10000000;
$cutoff = 5000000;
$sum = 10;
$count = -5;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $sum = $sum + $i;
    }
    $count = $count + 1;
}
$elapsed = microtime(true) - $t;
echo $sum . ',' . $count . '|' . $elapsed;
