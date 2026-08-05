<?php
// Holdout for an affine expression whose multiplier and bias are invariant CVs.
$n = 100000000;
$factor = 73;
$bias = 19;
$last = 0;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $last = ($i * $factor) + $bias;
    $sum = $sum + $last;
}
$elapsed = microtime(true) - $t;
echo $last . ',' . $sum . '|' . $elapsed;
