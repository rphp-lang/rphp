<?php
// Monomorphic scalar method whose proven body can compose into the native loop.
class ScalarKernel
{
    public function transform(int $value, int $scale): int
    {
        return ($value * $scale) + 7;
    }
}

$kernel = new ScalarKernel();
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < 10000000; $i++) {
    $sum += $kernel->transform($i, 73);
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
