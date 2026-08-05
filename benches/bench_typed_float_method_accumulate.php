<?php
// Holdout for composing a monomorphic typed-Double method into the caller
// loop. The method body is the same general five-operation expression used by
// the direct-function benchmark; only the guarded dispatch boundary differs.
class FloatCalculator
{
    public function calculate(
        float $a,
        float $b,
        float $c,
        float $d,
        float $e,
        float $f
    ): float {
        return (((($a + $b) * $c) - $d) * $e) + $f;
    }
}

$calculator = new FloatCalculator();
$total = 0.0;
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $total += $calculator->calculate(1.5, 2.5, 2.0, 1.0, 0.5, 2.5);
}
$elapsed = microtime(true) - $start;

echo $total . '|' . $elapsed;
