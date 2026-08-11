<?php

// Bound generic property mutation fed by an invariant typed JSON projection.
// Decode/projection validation belongs to the region prelude; the loop body
// should enter one native mixed region.
class GenericJsonPropertyNativeBox<T : int>
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

function runGenericJsonProperty(
    GenericJsonPropertyNativeBox $box,
    string $json,
    int $limit
): int {
    $checksum = 0;
    for ($i = 0; $i < $limit; $i++) {
        $row = json_decode($json, true);
        $value = $row['value'];
        $box->add($value);
        $checksum += $i;
    }
    return $box->total + $checksum;
}

$json = '{"value":7}';
runGenericJsonProperty(new GenericJsonPropertyNativeBox::<int>(0), $json, 1000);
$box = new GenericJsonPropertyNativeBox::<int>(0);
$start = microtime(true);
$sum = runGenericJsonProperty($box, $json, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
