<?php

// Holdout for a pure exact-Double leaf with two arithmetic return arms. Both
// branches are hot and the induction-dependent input prevents constant-folding
// the predicate outside the loop.
function conditionalFloat(float $value, float $pivot): float
{
    if ($value < $pivot) {
        return ($value * 1.5) + 2.0;
    }

    return ($value * 0.5) - 1.0;
}

$pivot = 1250000.0;
$total = 0.0;
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $total += conditionalFloat($i * 0.5, $pivot);
}
$elapsed = microtime(true) - $start;

echo $total . '|' . $elapsed;
