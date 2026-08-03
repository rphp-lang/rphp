<?php
// Nested pure scalar functions flattened into one guarded native call region.
function addNative($left, $right)
{
    return $left + $right;
}

function mulNative($left, $right)
{
    return $left * $right;
}

$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 5000000; $i++) {
    $sum += addNative($i + 1, mulNative($i, 2));
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
