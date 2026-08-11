<?php

// Bound generic property mutator. Method and property contracts are proven
// once before the native region; the timed loop contains no metadata lookup.
class GenericPropertyNativeCounter<T : int>
{
    public T $total;

    public function __construct(T $total)
    {
        $this->total = $total;
    }

    public function add(T $value)
    {
        $this->total = $this->total + $value;
    }
}

function runGenericPropertyNative(GenericPropertyNativeCounter $counter, int $limit): int
{
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $counter->add(1);
        $checksum += $i;
    }
    return $counter->total + $checksum;
}

$counter = new GenericPropertyNativeCounter::<int>(0);
runGenericPropertyNative($counter, 1000);
$start = microtime(true);
$sum = runGenericPropertyNative($counter, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
