<?php
// Holdout for target-neutral composition of one guarded typed-Double leaf.
// The outer function has never been flattened into a ScalarDoubleProgram by
// the compiler; the runtime must first validate the nested inline-cache target.
function scaleAndShift(float $value, float $scale): float
{
    return ($value * $scale) + 1.0;
}

function calculateNested(float $value, float $scale): float
{
    return (scaleAndShift($value, $scale) * 0.5) + 2.0;
}

$scale = 2.0;
$total = 0.0;
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $total += calculateNested($i * 0.5, $scale);
}
$elapsed = microtime(true) - $start;

echo $total . '|' . $elapsed;
