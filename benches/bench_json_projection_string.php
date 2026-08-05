<?php
// The exact String leaf is materialized once and strlen is derived as a Long
// projection, so the scalar loop performs no JSON, array or string work.
$json = '{"name":"hyper-optimized"}';
$iterations = 2000000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + strlen($row['name']);
}
$elapsed = microtime(true) - $start;
echo $sum . ':' . $row['name'] . '|' . $elapsed;
