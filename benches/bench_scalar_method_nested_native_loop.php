<?php

function scalarNestedAdd(int $left, int $right): int
{
    return $left + $right;
}

class ScalarNestedNativeKernel
{
    public function multiply(int $left, int $right): int
    {
        return $left * $right;
    }
}

$kernel = new ScalarNestedNativeKernel();
$runScalarNestedNative = function ($kernel, int $limit): int {
    $sum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += scalarNestedAdd($i, $kernel->multiply($i, 2));
    }
    return $sum;
};

$runScalarNestedNative($kernel, 1000);
$start = microtime(true);
$sum = $runScalarNestedNative($kernel, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
