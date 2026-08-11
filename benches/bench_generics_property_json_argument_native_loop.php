<?php

// Nested invariant JSON projections evaluated directly as property-method
// arguments. The compiler emits deferred argument evaluation after
// InitMethodCall; the complete body should still become one native region.
class GenericJsonArgumentNativeBox<T : int>
{
    public T $total;

    public function __construct(T $total)
    {
        $this->total = $total;
    }

    public function addPair(T $left, T $right): void
    {
        $this->total = $this->total + $left;
        $this->total = $this->total + $right;
    }
}

function runGenericJsonArgument(
    GenericJsonArgumentNativeBox $box,
    string $json,
    int $limit
): int {
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $row = json_decode($json, true);
        $box->addPair($row['left'], $row['nested']['right']);
        $checksum += $i;
    }
    return $box->total + $checksum;
}

$json = '{"left":3,"nested":{"right":4}}';
runGenericJsonArgument(new GenericJsonArgumentNativeBox::<int>(0), $json, 1000);
$box = new GenericJsonArgumentNativeBox::<int>(0);
$start = microtime(true);
$sum = runGenericJsonArgument($box, $json, 2000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
