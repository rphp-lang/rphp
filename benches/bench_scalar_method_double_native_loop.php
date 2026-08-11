<?php

class ScalarDoubleNativeKernel
{
    public function scale(float $value, float $factor): float
    {
        return $value * $factor;
    }
}

$kernel = new ScalarDoubleNativeKernel();
$runScalarDoubleNative = function ($kernel, int $limit): float {
    $sum = 0.0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += $kernel->scale(1.5, 2.0);
    }
    return $sum;
};

$runScalarDoubleNative($kernel, 1000);
$start = microtime(true);
$sum = $runScalarDoubleNative($kernel, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
