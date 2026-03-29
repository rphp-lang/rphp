<?php
// Nested loops: 1500x1500 = 2.25M iterations
$t = microtime(true);
$sum = 0;
for ($i = 0; $i < 1500; $i++) {
    for ($j = 0; $j < 1500; $j++) {
        $sum += $i + $j;
    }
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
