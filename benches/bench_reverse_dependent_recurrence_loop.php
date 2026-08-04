<?php
// The dependent update consumes the first CV before that CV is updated.
$n = 10000000;
$a = 3;
$b = -7;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $b = $b + $a;
    $a = $a + 1;
}
$elapsed = microtime(true) - $t;
echo $a . ',' . $b . '|' . $elapsed;
