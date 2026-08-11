<?php

// Ordinary-signature control for the generic conditional property workload.
class ScalarConditionalPropertyNativeSource
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

class ScalarConditionalPropertyNativeTarget
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

function runScalarConditionalProperty(
    ScalarConditionalPropertyNativeTarget $target,
    ScalarConditionalPropertyNativeSource $source,
    int $limit
): int {
    $selected = 0;
    for ($i = 0; $i < $limit; $i++) {
        $target->add($source->current());
        if (($i % 2) == 0) {
            $selected += $i;
        }
    }
    return $target->total + $selected;
}

$source = new ScalarConditionalPropertyNativeSource(7);
runScalarConditionalProperty(new ScalarConditionalPropertyNativeTarget(0), $source, 1000);
$target = new ScalarConditionalPropertyNativeTarget(0);
$start = microtime(true);
$sum = runScalarConditionalProperty($target, $source, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
