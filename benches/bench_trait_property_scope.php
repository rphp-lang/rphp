<?php
// Independent mixed-consumer control for ordinary trait property reads.
trait ScopeReader {
    public function readPeer($object): string { return $object->text; }
    public function ownText(): string { return $this->text; }
}
class ScopeParent {
    use ScopeReader;
    private string $text = 'parent';
}
class ScopeChild extends ScopeParent {
    use ScopeReader;
    private string $text = 'child';
}
class ScopeInherited extends ScopeParent {}
class ScopeRecomposed extends ScopeInherited {
    use ScopeReader;
    private string $text = 'recomposed';
}
$parent = new ScopeParent;
$child = new ScopeChild;
$inherited = new ScopeInherited;
$recomposed = new ScopeRecomposed;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < 500_000; $i++) {
    $sum += strlen($parent->readPeer($child));
    $sum += strlen($child->readPeer($child));
    $sum += strlen($inherited->readPeer($child));
    $sum += strlen($recomposed->ownText());
}
echo $sum, '|', microtime(true) - $start, "\n";
