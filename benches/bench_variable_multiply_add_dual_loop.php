<?php
// Two independent variable multiply-add results in one scalar body.
$n = 100000000;
$leftFactor = 73;
$leftBias = 19;
$rightFactor = 37;
$rightBias = 11;
$left = 0;
$right = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $left = ($i * $leftFactor) + $leftBias;
    $right = ($i * $rightFactor) + $rightBias;
}
$elapsed = microtime(true) - $t;
echo $left . ',' . $right . '|' . $elapsed;
