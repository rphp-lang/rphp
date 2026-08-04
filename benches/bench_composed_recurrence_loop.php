<?php
// Loop-carried Long state with an acyclic scalar expression as its delta.
$n = 10000000;
$sum = 10;
$offset = 7;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum = $sum + (($i * 3) + $offset);
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
