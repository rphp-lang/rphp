<?php
// Forward scalar if/else whose result feeds a composed expression.
$n = 10000000;
$cutoff = 5000000;
$selected = 0;
$folded = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $selected = ($i * 3) + 1;
    } else {
        $selected = ($i * 5) - 2;
    }
    $folded = ($selected * 3) + 11;
}
$elapsed = microtime(true) - $t;
echo $selected . ',' . $folded . '|' . $elapsed;
