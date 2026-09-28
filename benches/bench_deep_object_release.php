<?php
// Separate construction from final-owner release of an acyclic object chain.
// GC stays disabled to isolate ordinary reference-counted destruction.
gc_disable();

final class DeepReleaseNode
{
    public function __construct(public ?self $next, public int $value) {}
}

$count = (int) ($argv[1] ?? 2048);
if ($count < 1 || $count > 16384) {
    exit(2);
}

$started = microtime(true);
$head = null;
for ($i = 0; $i < $count; $i++) {
    $head = new DeepReleaseNode($head, $i);
}
$built = microtime(true);
$last = $head->value;
unset($head);
$released = microtime(true);

echo $count . ':' . $last . '|' . ($built - $started) . '|' . ($released - $built) . "\n";
