<?php
// Branch-dominated scalar expression used as a conditional recurrence delta.
$n = 10000000;
$cutoff = 4999995;
$offset = 7;
$sum = 10;
$count = -5;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    if ($count < $cutoff) {
        $sum = $sum + (($i * 3) + $offset);
    }
    $count = $count + 1;
}
$elapsed = microtime(true) - $t;
echo $sum . ',' . $count . '|' . $elapsed;
