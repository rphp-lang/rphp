<?php

// Ordinary-signature control for the composed generic property workload.
class ScalarComposedPropertyNativeSource
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

class ScalarComposedPropertyNativeTarget
{
    public int $total;

    public function __construct(int $total)
    {
        $this->total = $total;
    }

    public function add(int $value): void
    {
        $this->total = $this->total + $value;
    }
}

function runScalarComposedProperty(
    ScalarComposedPropertyNativeTarget $target,
    ScalarComposedPropertyNativeSource $source,
    int $limit
): int {
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $target->add($source->current());
        $checksum += $i;
    }
    return $target->total + $checksum;
}

$source = new ScalarComposedPropertyNativeSource(7);
runScalarComposedProperty(new ScalarComposedPropertyNativeTarget(0), $source, 1000);
$target = new ScalarComposedPropertyNativeTarget(0);
$start = microtime(true);
$sum = runScalarComposedProperty($target, $source, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
