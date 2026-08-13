<?php
// Repeatedly construct and release one ordinary declared object with three
// scalar property slots. This isolates owner and property-storage lifecycle.
class DeclaredLifecycleRow
{
    public $first = 17;
    public $second = 19;
    public $third = 23;
}

function run_declared_object_lifecycle($iterations)
{
    $checksum = 0;
    for ($index = 0; $index < $iterations; ++$index) {
        $row = new DeclaredLifecycleRow();
        $checksum += $row->first;
    }
    return $checksum;
}

$start = microtime(true);
$result = run_declared_object_lifecycle(1000000);
$elapsed = microtime(true) - $start;
echo $result, '|', $elapsed, "\n";
