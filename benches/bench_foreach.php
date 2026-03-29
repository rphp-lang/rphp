<?php
// Foreach iteration: 500K elements
$a = [];
for ($i = 0; $i < 500000; $i++) {
    $a[] = $i;
}
$t = microtime(true);
$sum = 0;
foreach ($a as $v) {
    $sum += $v;
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
