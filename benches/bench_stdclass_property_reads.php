<?php
// Isolate canonical dynamic stdClass reads from JSON parsing and allocation.
$row = json_decode('{"value":11,"name":"alpha"}');
$iterations = 5000000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $sum += $row->value + strlen($row->name);
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
