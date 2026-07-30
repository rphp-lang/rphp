<?php
// Sequential reads from an immutable packed array.
$values = range(1, 1000000);
$n = count($values);
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum += $values[$i];
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
