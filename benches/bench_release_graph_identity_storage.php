<?php
final class GraphObservedOwner {
    public static $released = 0;

    public function __destruct() {
        ++self::$released;
    }
}

final class GraphIdentityLeaf {
    public static $released = 0;
    public $id;

    public function __destruct() {
        ++self::$released;
    }
}

function consumeReleaseGraphRow($width, $seed, $shared) {
    $row = [];
    for ($i = 0; $i < $width; $i++) {
        $leaf = new GraphIdentityLeaf;
        $leaf->id = $seed + $i;
        $row[] = [$leaf, $shared];
    }
    $checksum = 0;
    foreach ($row as $branch) {
        $checksum += $branch[0]->id + $branch[1]->id;
    }
    unset($branch);
    return $checksum;
}

$shared = new stdClass;
$shared->id = 3;
$observed = new GraphObservedOwner;
$checksum = 0;
$start = microtime(true);
foreach ([1, 4, 12, 64] as $width) {
    for ($round = 0; $round < 5_000; $round++) {
        $checksum += consumeReleaseGraphRow($width, $round & 15, $shared);
    }
}
unset($observed);
echo $checksum, ':', GraphObservedOwner::$released, ':', GraphIdentityLeaf::$released,
    '|', microtime(true) - $start, "\n";
