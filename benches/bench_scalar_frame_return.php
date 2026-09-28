<?php
// Scalar-argument control with the same setup and caller-frame layout as
// bench_shared_frame_release.php. Setup and teardown remain outside the timer.
class SharedFrameReleaseValue
{
    public function __construct(public int $number) {}
}

function readSharedFrame($object, $items, $closure, $choose)
{
    $alias = $object;
    $copy = $items;
    return $choose ? $alias : $copy;
}

$first = new SharedFrameReleaseValue(7);
$second = new SharedFrameReleaseValue(11);
$items = [$second];
$closure = static fn () => $first;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < 500000; $i++) {
    $sum += readSharedFrame(7, 11, 13, $i & 1);
}
echo $sum, '|', microtime(true) - $start, "\n";
