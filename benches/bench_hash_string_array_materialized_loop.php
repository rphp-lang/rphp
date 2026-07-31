<?php
// Invariant string-key reads with an explicitly materialized loop value.
$values = ['hot' => 7];
$n = 10000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $value = $values['hot'];
    $sum += $value;
}
$elapsed = microtime(true) - $t;
echo $sum . ':' . $value . '|' . $elapsed;
