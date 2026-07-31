<?php
// Sequential integer-key reads from hash storage with an explicitly
// materialized loop value.
$values = range(1, 1000000);
$values['sentinel'] = 0;
$n = 1000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $value = $values[$i];
    $sum += $value;
}
$elapsed = microtime(true) - $t;
echo $sum . ':' . $value . '|' . $elapsed;
