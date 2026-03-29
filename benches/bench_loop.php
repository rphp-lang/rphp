<?php
// Tight loop: 10M iterations of simple arithmetic
$t = microtime(true);
$sum = 0;
for ($i = 0; $i < 10000000; $i++) {
    $sum += $i;
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
