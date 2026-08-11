<?php

class GenericDoubleNativeKernel<T>
{
    public function scale(T $value, float $factor): T
    {
        return $value * $factor;
    }
}

$kernel = new GenericDoubleNativeKernel::<float>();
$runGenericDoubleNative = function ($kernel, int $limit): float {
    $sum = 0.0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += $kernel->scale(1.5, 2.0);
    }
    return $sum;
};

$runGenericDoubleNative($kernel, 1000);
$start = microtime(true);
$sum = $runGenericDoubleNative($kernel, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
