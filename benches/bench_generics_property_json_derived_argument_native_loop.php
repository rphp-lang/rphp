<?php

// A fixed invariant JSON Long plus a nested String byte length evaluated
// directly as generic property-method arguments. Both derived inputs should
// be published by the prelude and consumed by one native region.
class GenericJsonDerivedArgumentNativeBox<T : int>
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

function runGenericJsonDerivedArgument(
    GenericJsonDerivedArgumentNativeBox $box,
    string $json,
    int $limit
): int {
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $row = json_decode($json, true);
        $box->addPair($row['value'], strlen($row['nested']['name']));
        $checksum += $i;
    }
    return $box->total + $checksum;
}

$json = '{"value":3,"nested":{"name":"hyper-optimized"}}';
runGenericJsonDerivedArgument(new GenericJsonDerivedArgumentNativeBox::<int>(0), $json, 1000);
$box = new GenericJsonDerivedArgumentNativeBox::<int>(0);
$start = microtime(true);
$sum = runGenericJsonDerivedArgument($box, $json, 2000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
