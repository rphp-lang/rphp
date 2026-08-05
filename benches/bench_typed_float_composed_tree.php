<?php
// Holdout for bounded recursive composition of guarded typed-Double calls.
// Both outer functions contain a call and therefore require runtime target
// resolution before their complete arithmetic tree can enter one native region.
function scaleAndShift(float $value, float $scale): float
{
    return ($value * $scale) + 1.0;
}

function calculateNested(float $value, float $scale): float
{
    return (scaleAndShift($value, $scale) * 0.5) + 2.0;
}

function calculateOuter(float $value, float $scale): float
{
    return calculateNested($value, $scale) + 3.0;
}

$scale = 2.0;
$total = 0.0;
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $total += calculateOuter($i * 0.5, $scale);
}
$elapsed = microtime(true) - $start;

echo $total . '|' . $elapsed;
