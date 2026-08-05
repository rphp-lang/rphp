<?php
// Invariant associative JSON with fixed Long projections. RPHP may decode the
// document once, guard every projected path and feed the scalar loop directly.
$json = '{"name":"Alice","age":30,"scores":[95,87,92],"address":{"house":14}}';
$iterations = 2000000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + $row['age'] + $row['scores'][0] + $row['scores'][1]
        + $row['scores'][2] + $row['address']['house'];
}
$elapsed = microtime(true) - $start;
echo $sum . ':' . $row['name'] . '|' . $elapsed;
