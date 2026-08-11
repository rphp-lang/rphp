<?php

// Bound generic composed property call beside a fused conditional recurrence.
// The complete body should remain in one native mixed region.
class GenericConditionalPropertyNativeSource<T : int>
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

class GenericConditionalPropertyNativeTarget<T : int>
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

function runGenericConditionalProperty(
    GenericConditionalPropertyNativeTarget $target,
    GenericConditionalPropertyNativeSource $source,
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

$source = new GenericConditionalPropertyNativeSource::<int>(7);
runGenericConditionalProperty(
    new GenericConditionalPropertyNativeTarget::<int>(0),
    $source,
    1000
);
$target = new GenericConditionalPropertyNativeTarget::<int>(0);
$start = microtime(true);
$sum = runGenericConditionalProperty($target, $source, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
