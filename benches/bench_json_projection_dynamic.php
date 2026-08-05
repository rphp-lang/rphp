<?php
// Control workload: the JSON input changes in the loop, so invariant projection
// fusion must reject it and preserve canonical per-iteration decoding.
$first = '{"value":11}';
$second = '{"value":17}';
$json = $first;
$iterations = 200000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    if (($i % 2) == 0) {
        $json = $first;
    } else {
        $json = $second;
    }
    $row = json_decode($json, true);
    $sum = $sum + $row['value'];
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
