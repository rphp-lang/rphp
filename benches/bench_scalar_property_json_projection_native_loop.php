<?php

// Ordinary-signature control for the JSON-to-property native workload.
class ScalarJsonPropertyNativeBox
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

function runScalarJsonProperty(
    ScalarJsonPropertyNativeBox $box,
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
runScalarJsonProperty(new ScalarJsonPropertyNativeBox(0), $json, 1000);
$box = new ScalarJsonPropertyNativeBox(0);
$start = microtime(true);
$sum = runScalarJsonProperty($box, $json, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
