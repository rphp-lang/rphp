<?php

function genericNestedAdd(int $left, int $right): int
{
    return $left + $right;
}

class GenericNestedNativeKernel<T>
{
    public function multiply(T $left, int $right): T
    {
        return $left * $right;
    }
}

$kernel = new GenericNestedNativeKernel::<int>();
$runGenericNestedNative = function ($kernel, int $limit): int {
    $sum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += genericNestedAdd($i, $kernel->multiply($i, 2));
    }
    return $sum;
};

$runGenericNestedNative($kernel, 1000);
$start = microtime(true);
$sum = $runGenericNestedNative($kernel, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
