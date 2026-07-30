<?php
// Closed scalar loop used to measure baseline dispatch and quick-region execution.
$n = 50000000;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum += $i + 1;
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
