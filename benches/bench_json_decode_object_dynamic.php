<?php
// Canonical object-mode control: the input changes in the loop and every
// decoded object is consumed through ordinary stdClass property reads.
$first = '{"value":11,"name":"alpha"}';
$second = '{"value":17,"name":"longer"}';
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
    $row = json_decode($json);
    $sum += $row->value + strlen($row->name);
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
