<?php

// Ordinary-signature control for the generic property-getter workload.
class ScalarPropertyNativeSource
{
    public int $value;

    public function __construct(int $value)
    {
        $this->value = $value;
    }

    public function current(): int
    {
        return $this->value;
    }
}

function runScalarPropertyGetter(ScalarPropertyNativeSource $source, int $limit): int
{
    $sum = 0;
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += $source->current();
        $checksum += $i;
    }
    return $sum + $checksum;
}

$source = new ScalarPropertyNativeSource(7);
runScalarPropertyGetter($source, 1000);
$start = microtime(true);
$sum = runScalarPropertyGetter($source, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
