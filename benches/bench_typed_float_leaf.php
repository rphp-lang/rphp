<?php
// Typed Double leaf kept at one stable call site. The body is deliberately a
// general five-operation scalar expression rather than a loop-specific kernel.
function calculateFloat(
    float $a,
    float $b,
    float $c,
    float $d,
    float $e,
    float $f
): float {
    return (((($a + $b) * $c) - $d) * $e) + $f;
}

$total = 0.0;
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $total += calculateFloat(1.5, 2.5, 2.0, 1.0, 0.5, 2.5);
}
$elapsed = microtime(true) - $start;

echo $total . '|' . $elapsed;
