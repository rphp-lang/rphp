<?php
// A cold arbitrary PHP edge remains outside one range-proven native loop.
$limit = 10000000;
$needle = -1;
$sum = 0;

for ($i = 0; $i < $limit; $i++) {
    $sum += $i;
    if ($i === $needle) {
        echo "unreachable\n";
    }
}

echo $i . ':' . $sum . "\n";
