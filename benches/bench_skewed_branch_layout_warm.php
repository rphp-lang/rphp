<?php
// Warmed form of the 90/10 diamond. The first call builds the native region;
// the timed second call isolates steady-state generated-code throughput.
function skewedBranchLayout(int $n, int $cutoff): int
{
    $selected = 0;
    $folded = 0;
    for ($i = 0; $i < $n; $i++) {
        if ($i < $cutoff) {
            $selected = ($i * 3) + 1;
        } else {
            $selected = ($i * 5) - 2;
        }
        $folded = ($selected * 3) + 11;
    }
    return $folded;
}

skewedBranchLayout(100000, 90000);
$t = microtime(true);
$result = skewedBranchLayout(10000000, 9000000);
$elapsed = microtime(true) - $t;
echo $result . '|' . $elapsed;
