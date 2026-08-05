<?php

// Holdout for a conditional exact-Double leaf embedded in a larger composed
// call tree. The outer suffix consumes the selected value, so flattening must
// merge the two edges before continuing instead of executing both arms.
function conditionalLeaf(float $value, float $pivot): float
{
    if ($value < $pivot) {
        return ($value * 1.5) + 2.0;
    }

    return ($value * 0.5) - 1.0;
}

function composedConditional(float $value, float $pivot): float
{
    return (conditionalLeaf($value, $pivot) * 1.25) + 3.0;
}

$pivot = 1250000.0;
$total = 0.0;
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $total += composedConditional($i * 0.5, $pivot);
}
$elapsed = microtime(true) - $start;

echo $total . '|' . $elapsed;
