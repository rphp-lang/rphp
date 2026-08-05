<?php
// Independent variable multiply-add result: isolates code size and throughput.
$n = 100000000;
$factor = 73;
$bias = 19;
$last = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $last = ($i * $factor) + $bias;
}
$elapsed = microtime(true) - $t;
echo $last . '|' . $elapsed;
