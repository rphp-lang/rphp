<?php
// The caller computes one induction-dependent and one invariant Double
// expression before entering the same general five-operation typed leaf used
// by bench_typed_float_leaf.php.
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

$scale = 2.0;
$total = 0.0;
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $total += calculateFloat($i * 0.5, $scale + 1.0, 2.0, 1.0, 0.5, 2.5);
}
$elapsed = microtime(true) - $start;

echo $total . '|' . $elapsed;
