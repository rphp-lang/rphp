<?php
// Two loop-invariant scalar function arguments that cannot be constant-folded.
function runTwoInvariantLoop(int $n, int $cutoff, int $offset): int
{
    $selected = 0;
    $folded = 0;
    for ($i = 0; $i < $n; $i++) {
        if ($i < $cutoff) {
            $selected = ($i * 3) + $offset;
        } else {
            $selected = ($i * 5) - $offset;
        }
        $folded = ($selected * 3) + $offset;
    }
    return $selected + $folded;
}

$t = microtime(true);
$result = runTwoInvariantLoop(10000000, 5000000, 7);
$elapsed = microtime(true) - $t;
echo $result . '|' . $elapsed;
