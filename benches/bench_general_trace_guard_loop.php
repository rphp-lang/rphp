<?php

// General typed region with two independent loop-carried values and one
// structurally cold arbitrary PHP edge.
$n = 10000000;
$needle = -1;
$sum = 0;
$count = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum = $sum + $i;
    $count = $count + 1;
    if ($count === $needle) {
        echo 'never';
    }
}
$elapsed = microtime(true) - $t;
echo $sum . ':' . $count . '|' . $elapsed;
