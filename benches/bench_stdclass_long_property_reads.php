<?php
$row = json_decode('{"value":11}');
$iterations = 10000000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $sum += $row->value;
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
