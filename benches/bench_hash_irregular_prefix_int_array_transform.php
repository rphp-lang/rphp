<?php
// Regular sparse scan in an array whose leading keys reject the stride hint.
$values = [];
$values[11] = -1;
$values[30] = -1;
$values[31] = -1;
$values[70] = -1;
$values[-4] = -1;
$values[900] = -1;
$values[2] = -1;
$values[88] = -1;
$start = 1000000;
$n = 1000000;
$stride = 7;
$key = $start;
for ($i = 0; $i < $n; $i++) {
    $values[$key] = $i;
    $key = $key + $stride;
}
$one = 1;
$key = $start;
$sum = 0;
$adjusted = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $value = $values[$key];
    $sum += $value;
    $adjusted += $value + $one;
    $key = $key + $stride;
}
$elapsed = microtime(true) - $t;
echo $sum . ':' . $adjusted . '|' . $elapsed;
