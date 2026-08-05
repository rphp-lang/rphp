<?php
// Negative control for the typed projections: the input changes inside the
// loop, so the invariant source must be rejected for both Double and String.
function scaleJsonProjectionDynamicBench(float $value): float
{
    return $value * 1.5;
}

$first = '{"value":1.25,"name":"alpha"}';
$second = '{"value":2.5,"name":"longer"}';
$json = $first;
$iterations = 200000;
$sum = 0.0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    if (($i % 2) == 0) {
        $json = $first;
    } else {
        $json = $second;
    }
    $row = json_decode($json, true);
    $sum += scaleJsonProjectionDynamicBench($row['value']) + strlen($row['name']);
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
