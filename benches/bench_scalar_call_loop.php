<?php
// Pure scalar user call retained inside a quickened accumulation loop.
function calculate($x)
{
    return $x * 2 + 1;
}

$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 10000000; $i++) {
    $sum += calculate($i);
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
