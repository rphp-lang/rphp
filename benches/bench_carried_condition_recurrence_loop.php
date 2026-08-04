<?php
// A recurrence guard reads another recurrence's current fixed-register value.
$n = 10000000;
$cutoff = 4999995;
$sum = 10;
$count = -5;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    if ($count < $cutoff) {
        $sum = $sum + $i;
    }
    $count = $count + 1;
}
$elapsed = microtime(true) - $t;
echo $sum . ',' . $count . '|' . $elapsed;
