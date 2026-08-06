<?php
// Structural control: sparse integer insertion must not pay for contiguous
// packed-to-hash read metadata on every append.
$start = 1000000;
$stride = 7;
$n = 1000000;
$values = [];
$key = $start;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $values[$key] = $i;
    $key += $stride;
}
$elapsed = microtime(true) - $t;
echo count($values) . ':' . $key . '|' . $elapsed;
