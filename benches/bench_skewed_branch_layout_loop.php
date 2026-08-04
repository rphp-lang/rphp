<?php
// Unseen 90/10 forward diamond: the dominant true arm should remain the
// physical fallthrough while the cold false arm is outlined by the x86 JIT.
$n = 10000000;
$cutoff = 9000000;
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
