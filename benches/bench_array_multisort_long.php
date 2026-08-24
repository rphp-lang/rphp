<?php

$count = 150000;
$primary = [];
$secondary = [];
for ($index = 0; $index < $count; $index++) {
    $primary[] = ($index * 48271) & 4095;
    $secondary[] = $count - $index;
}

$startedAt = microtime(true);
$sorted = array_multisort(
    $primary,
    SORT_ASC,
    SORT_NUMERIC,
    $secondary,
    SORT_DESC,
    SORT_NUMERIC,
);
$elapsed = microtime(true) - $startedAt;

$last = $count - 1;
$checksum = ($sorted ? 1 : 0)
    + count($primary)
    + count($secondary)
    + $primary[0]
    + $primary[$last]
    + $secondary[0]
    + $secondary[$last];
echo $checksum . "|" . $elapsed;
