<?php
// The same acyclic chain as bench_deep_object_release, with PHP destructors.
gc_disable();
$releasedNodes = 0;

final class DeepDestructorNode
{
    public function __construct(public ?self $next, public int $value) {}

    public function __destruct()
    {
        $GLOBALS['releasedNodes']++;
    }
}

$count = (int) ($argv[1] ?? 2048);
if ($count < 1 || $count > 16384) {
    exit(2);
}

$started = microtime(true);
$head = null;
for ($i = 0; $i < $count; $i++) {
    $head = new DeepDestructorNode($head, $i);
}
$built = microtime(true);
$last = $head->value;
unset($head);
$released = microtime(true);

echo $count . ':' . $last . ':' . $releasedNodes . '|' . ($built - $started) . '|' . ($released - $built) . "\n";
