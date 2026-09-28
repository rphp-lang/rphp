<?php
// Repeated ordinary calls borrowing retained object/array/closure owners.
// The timed loop excludes setup and final-owner destruction.
class SharedFrameReleaseValue
{
    public function __construct(public int $number) {}
}

function readSharedFrame($object, $items, $closure, $choose)
{
    $alias = $object;
    $copy = $items;
    return $choose ? $alias->number : $copy[0]->number;
}

$first = new SharedFrameReleaseValue(7);
$second = new SharedFrameReleaseValue(11);
$items = [$second];
$closure = static fn () => $first;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < 500000; $i++) {
    $sum += readSharedFrame($first, $items, $closure, $i & 1);
}
echo $sum, '|', microtime(true) - $start, "\n";
