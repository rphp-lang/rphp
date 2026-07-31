<?php
// General string lookup when the selected key comes from another CV.
$values = ['left' => 3, 'right' => 5];
$left = 'left';
$right = 'right';
$key = $left;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < 1000000; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = $right;
    } else {
        $key = $left;
    }
}
$elapsed = microtime(true) - $t;
echo $sum . ':' . $key . '|' . $elapsed;
