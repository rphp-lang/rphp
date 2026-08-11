<?php

class GenericNestedDoubleNativeKernel<T>
{
    public function scale(T $value, float $factor): T
    {
        return $value * $factor;
    }

    public function composed(float $value, float $factor): float
    {
        return $this->scale($value, $factor) + 1.0;
    }
}

$kernel = new GenericNestedDoubleNativeKernel::<float>();
$runGenericNestedDoubleNative = function ($kernel, int $limit): float {
    $sum = 0.0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += $kernel->composed(1.5, 2.0);
    }
    return $sum;
};

$runGenericNestedDoubleNative($kernel, 1000);
$start = microtime(true);
$sum = $runGenericNestedDoubleNative($kernel, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
