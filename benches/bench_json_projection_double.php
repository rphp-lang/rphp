<?php
// Invariant associative JSON with one exact Double projection feeding the
// existing target-neutral scalar-call program and Double accumulation loop.
function scaleJsonProjectionBench(float $value): float
{
    return $value * 1.5;
}

$json = '{"value":1.25}';
$iterations = 2000000;
$sum = 0.0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $row = json_decode($json, true);
    $sum += scaleJsonProjectionBench($row['value']);
}
$elapsed = microtime(true) - $start;
echo $sum . ':' . $row['value'] . '|' . $elapsed;
