<?php

class GenericComposedLongKernel<T>
{
    public function step(T $value): T
    {
        return $value + 1;
    }
}

function consumeGenericComposedLong(GenericComposedLongKernel $kernel, int $value): int
{
    return ($kernel->step($value) % 97) ^ 13;
}

$kernel = new GenericComposedLongKernel::<int>();
$runGenericComposedLong = function ($kernel, int $limit): int {
    $sum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += consumeGenericComposedLong($kernel, $i);
    }
    return $sum;
};

$runGenericComposedLong($kernel, 1000);
$start = microtime(true);
$sum = $runGenericComposedLong($kernel, 2000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
