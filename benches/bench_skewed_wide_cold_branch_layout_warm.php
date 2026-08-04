<?php
// Warmed 90/10 diamond with a wider cold expression. Its physical removal
// keeps the downstream join in approximately the same x86 op-cache phase.
function skewedWideColdBranchLayout(int $n, int $cutoff): int
{
    $selected = 0;
    $folded = 0;
    for ($i = 0; $i < $n; $i++) {
        if ($i < $cutoff) {
            $selected = ($i * 3) + 1;
        } else {
            $selected = ((($i * 5) - 2) * 3) + 7;
        }
        $folded = ($selected * 3) + 11;
    }
    return $folded;
}

skewedWideColdBranchLayout(100000, 90000);
$t = microtime(true);
$result = skewedWideColdBranchLayout(10000000, 9000000);
$elapsed = microtime(true) - $t;
echo $result . '|' . $elapsed;
