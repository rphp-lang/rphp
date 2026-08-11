<?php

// Ordinary-signature control for direct nested JSON method arguments.
class ScalarJsonArgumentNativeBox
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

function runScalarJsonArgument(
    ScalarJsonArgumentNativeBox $box,
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
runScalarJsonArgument(new ScalarJsonArgumentNativeBox(0), $json, 1000);
$box = new ScalarJsonArgumentNativeBox(0);
$start = microtime(true);
$sum = runScalarJsonArgument($box, $json, 2000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
