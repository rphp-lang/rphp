<?php
$first = '{"a":1,"b":2,"c":3,"d":4}';
$second = '{"a":2,"b":3,"c":4,"d":5}';
$iterations = 200000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $json = (($i % 2) == 0) ? $first : $second;
    $row = json_decode($json);
    $sum += $row->d;
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
