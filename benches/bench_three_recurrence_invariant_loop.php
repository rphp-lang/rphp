<?php
// Three carried values leave only the constant-bound register available for
// a shared runtime invariant in the x86-64 polling-native entry.
$offset = 5;
$left = 1;
$middle = 2;
$right = 3;

$started = microtime(true);
for ($i = 0; $i < 10000000; $i++) {
    $left = $left + $offset;
    $middle = $middle + $offset;
    $right = $right + $offset;
}
$elapsed = microtime(true) - $started;

echo $i . ':' . $left . ':' . $middle . ':' . $right . '|' . $elapsed;
