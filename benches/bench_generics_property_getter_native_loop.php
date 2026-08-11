<?php

// Bound generic property getter. The receiver/method/property proof and
// shadow-slot seed happen once before the timed native region.
class GenericPropertyNativeSource<T : int>
{
    public T $value;

    public function __construct(T $value)
    {
        $this->value = $value;
    }

    public function current(): T
    {
        return $this->value;
    }
}

function runGenericPropertyGetter(GenericPropertyNativeSource $source, int $limit): int
{
    $sum = 0;
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $sum += $source->current();
        $checksum += $i;
    }
    return $sum + $checksum;
}

$source = new GenericPropertyNativeSource::<int>(7);
runGenericPropertyGetter($source, 1000);
$start = microtime(true);
$sum = runGenericPropertyGetter($source, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
