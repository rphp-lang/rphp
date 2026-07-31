<?php
// Invariant integer-key reads with an explicitly materialized loop value.
$values = [7 => 9, 'sentinel' => 0];
$n = 10000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $value = $values[7];
    $sum += $value;
}
$elapsed = microtime(true) - $t;
echo $sum . ':' . $value . '|' . $elapsed;
