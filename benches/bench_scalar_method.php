<?php
// Scalar-return method: pure computation via method, 5M iterations
// Isolates method dispatch overhead (no property access in hot loop)
class Math {
    public function add($a, $b) { return $a + $b; }
    public function mul($a, $b) { return $a * $b; }
}

$m = new Math();
$t = microtime(true);
$sum = 0;
for ($i = 0; $i < 5000000; $i++) {
    $sum += $m->add($i, $m->mul($i, 2));
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
