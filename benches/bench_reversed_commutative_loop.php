<?php
// Constants on the left of commutative scalar operations exercise lowering normalization.
$n = 10000000;
$cutoff = 5000000;
$selected = 0;
$folded = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $selected = 1 + (3 * $i);
    } else {
        $selected = (5 * $i) - 2;
    }
    $folded = 11 + (3 * $selected);
}
$elapsed = microtime(true) - $t;
echo $selected . ',' . $folded . '|' . $elapsed;
