<?php
// Scalar values stay live across later assignments and are consumed again.
$n = 10000000;
$a = 0;
$b = 0;
$c = 0;
$d = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $a = $i * 3;
    $b = $a + 7;
    $c = $a + $b;
    $d = $a + $b + $c;
}
$elapsed = microtime(true) - $t;
echo $a . ',' . $b . ',' . $c . ',' . $d . '|' . $elapsed;
