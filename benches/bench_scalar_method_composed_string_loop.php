<?php

class ScalarComposedStringKernel
{
    public function label(int $value): string
    {
        if (($value & 1) === 0) {
            return 'even';
        }
        return 'odd';
    }
}

function consumeScalarComposedString(ScalarComposedStringKernel $kernel, int $value): int
{
    return strlen($kernel->label($value));
}

$kernel = new ScalarComposedStringKernel();
$runScalarComposedString = function ($kernel, int $limit): int {
    $sum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += consumeScalarComposedString($kernel, $i);
    }
    return $sum;
};

$runScalarComposedString($kernel, 1000);
$start = microtime(true);
$sum = $runScalarComposedString($kernel, 2000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
