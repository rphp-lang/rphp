<?php

// Bound generic property getter nested into a property mutator. Both receiver
// contracts and property slots are proven once before the native region.
class GenericComposedPropertyNativeSource<T : int>
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

class GenericComposedPropertyNativeTarget<T : int>
{
    public T $total;

    public function __construct(T $total)
    {
        $this->total = $total;
    }

    public function add(T $value): void
    {
        $this->total = $this->total + $value;
    }
}

function runGenericComposedProperty(
    GenericComposedPropertyNativeTarget $target,
    GenericComposedPropertyNativeSource $source,
    int $limit
): int {
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $target->add($source->current());
        $checksum += $i;
    }
    return $target->total + $checksum;
}

$source = new GenericComposedPropertyNativeSource::<int>(7);
runGenericComposedProperty(
    new GenericComposedPropertyNativeTarget::<int>(0),
    $source,
    1000
);
$target = new GenericComposedPropertyNativeTarget::<int>(0);
$start = microtime(true);
$sum = runGenericComposedProperty($target, $source, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
