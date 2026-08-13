<?php
// Repeatedly construct and release one ordinary declared object with three
// scalar property slots. This isolates owner and property-storage lifecycle.
class DeclaredLifecycleRow
{
    public $first = 1;
    public $second = 2;
    public $third = 3;

    public function __construct($seed)
    {
        $this->first = $seed;
        $this->second = $seed + 1;
        $this->third = $seed + 2;
    }
}

function run_declared_object_lifecycle($iterations)
{
    $checksum = 0;
    for ($index = 0; $index < $iterations; ++$index) {
        $row = new DeclaredLifecycleRow($index);
        $checksum += $row->first + $row->third;
    }
    return $checksum;
}

$start = microtime(true);
$result = run_declared_object_lifecycle(1000000);
$elapsed = microtime(true) - $start;
echo $result, '|', $elapsed, "\n";
