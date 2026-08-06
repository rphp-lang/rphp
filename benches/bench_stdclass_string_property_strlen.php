<?php
$row = json_decode('{"name":"alpha"}');
$iterations = 10000000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $sum += strlen($row->name);
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
