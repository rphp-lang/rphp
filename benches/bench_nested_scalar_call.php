<?php
// Nested quick loop consuming a pure scalar function of both induction values.
function combine($outer, $inner)
{
    return $outer * 2 + $inner;
}

$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 1500; $i++) {
    for ($j = 0; $j < 1500; $j++) {
        $sum += combine($i, $j);
    }
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
