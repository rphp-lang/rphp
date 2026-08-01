<?php
// Triangular nested loops: the inner bound changes with every outer iteration.
$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 3000; $i++) {
    for ($j = 0; $j < $i; $j++) {
        $sum += $i + $j;
    }
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
