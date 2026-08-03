<?php
// Conditional scalar calls in a loop whose strict-comparison/echo edge keeps
// the whole loop outside native region composition. This isolates the hot
// per-call scalar JIT while retaining ordinary, statically resolved PHP calls.
function routeStandalone(int $value): int
{
    if (($value & 1) == 0) {
        return ($value * 3) + 1;
    }
    return ($value * 5) - 2;
}

$sum = 0;
$start = microtime(true);
for ($i = 0; $i < 10000000; $i++) {
    $sum += routeStandalone($i);
    if ($i === -1) {
        echo 'never';
    }
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
