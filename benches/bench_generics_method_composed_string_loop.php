<?php

class GenericComposedStringKernel<T>
{
    public function label(T $value): string
    {
        if (($value & 1) === 0) {
            return 'even';
        }
        return 'odd';
    }
}

function consumeGenericComposedString(GenericComposedStringKernel $kernel, int $value): int
{
    return strlen($kernel->label($value));
}

$kernel = new GenericComposedStringKernel::<int>();
$runGenericComposedString = function ($kernel, int $limit): int {
    $sum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += consumeGenericComposedString($kernel, $i);
    }
    return $sum;
};

$runGenericComposedString($kernel, 1000);
$start = microtime(true);
$sum = $runGenericComposedString($kernel, 2000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
