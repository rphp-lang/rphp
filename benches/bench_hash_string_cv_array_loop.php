<?php
// Repeated immutable lookup using a string key held in a runtime CV.
$values = ['hot' => 7];
$key = 'hot';
$n = 10000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum += $values[$key];
}
$elapsed = microtime(true) - $t;
echo $sum . ':' . $key . '|' . $elapsed;
