<?php
// Irregular integer-index construction with most entropy above bit 32. This
// controls the integer hasher's high-to-low diffusion rather than favoring the
// low-bit permutation used by the primary irregular benchmark.
$n = 500000;
$values = [];
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $key = ($i << 32) | (($i * $i) & 1048575);
    $values[$key] = $i;
}
$elapsed = microtime(true) - $t;
echo $values[$key] . '|' . $elapsed;
