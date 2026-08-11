<?php

// Ordinary-signature control for a fixed JSON Long plus nested String byte
// length evaluated directly as property-method arguments.
class ScalarJsonDerivedArgumentNativeBox
{
    public int $total;

    public function __construct(int $total)
    {
        $this->total = $total;
    }

    public function addPair(int $left, int $right): void
    {
        $this->total = $this->total + $left;
        $this->total = $this->total + $right;
    }
}

function runScalarJsonDerivedArgument(
    ScalarJsonDerivedArgumentNativeBox $box,
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
runScalarJsonDerivedArgument(new ScalarJsonDerivedArgumentNativeBox(0), $json, 1000);
$box = new ScalarJsonDerivedArgumentNativeBox(0);
$start = microtime(true);
$sum = runScalarJsonDerivedArgument($box, $json, 2000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
