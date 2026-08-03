<?php
// Previously unseen straight body with non-materialized TMP arithmetic chains.
$n = 10000000;
$left = 2;
$right = 3;
$literal = 0;
$cv = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $literal = (($i * 73) + 20) - 7;
    $cv = $i + $left + $right;
}
$elapsed = microtime(true) - $t;
echo $literal . ',' . $cv . '|' . $elapsed;
