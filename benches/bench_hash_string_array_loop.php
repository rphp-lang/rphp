<?php
// Repeated immutable lookup by a string key in hash storage.
$values = ['hot' => 7];
$n = 10000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum += $values['hot'];
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
