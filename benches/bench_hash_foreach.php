<?php
// Value-only foreach after a string key forces ordered hash storage.
$values = range(0, 499999);
$values['tail'] = 7;
$start = microtime(true);
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
