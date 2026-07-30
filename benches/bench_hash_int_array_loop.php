<?php
// Sequential integer-key reads after forcing the array into hash storage.
$values = range(1, 1000000);
$values['sentinel'] = 0;
$n = 1000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum += $values[$i];
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
