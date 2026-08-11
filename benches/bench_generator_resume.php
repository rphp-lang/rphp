<?php

function values(int $limit)
{
    for ($i = 0; $i < $limit; $i++) {
        yield $i;
    }
}

$start = microtime(true);
$sum = 0;
foreach (values(200000) as $value) {
    $sum += $value;
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
