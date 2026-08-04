<?php
// The second loop-carried Long consumes the first one's updated value.
$n = 10000000;
$a = 3;
$b = -7;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $a = $a + 1;
    $b = $b + $a;
}
$elapsed = microtime(true) - $t;
echo $a . ',' . $b . '|' . $elapsed;
