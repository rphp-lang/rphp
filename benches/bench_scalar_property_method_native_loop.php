<?php

// Ordinary-signature control for the generic property-mutator workload.
class ScalarPropertyNativeCounter
{
    public int $total;

    public function __construct(int $total)
    {
        $this->total = $total;
    }

    public function add(int $value)
    {
        $this->total = $this->total + $value;
    }
}

function runScalarPropertyNative(ScalarPropertyNativeCounter $counter, int $limit): int
{
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $counter->add(1);
        $checksum += $i;
    }
    return $counter->total + $checksum;
}

$counter = new ScalarPropertyNativeCounter(0);
runScalarPropertyNative($counter, 1000);
$start = microtime(true);
$sum = runScalarPropertyNative($counter, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
