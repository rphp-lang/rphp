<?php
trait PeerReader {
    public function peer($object) { return $object->value; }
    public function own() { return $this->value; }
    public function items() { return $this->items; }
    public function push() { $this->items[] = count($this->items); }
}
class ParentOwner {
    use PeerReader;
    private string $value = 'parent';
    private array $items = [10];
}
class ChildOwner extends ParentOwner {
    use PeerReader;
    private string $value = 'child';
    private array $items = [20];
}
class InheritedOwner extends ParentOwner {}
class RecomposedOwner extends InheritedOwner {
    use PeerReader;
    private string $value = 'recomposed';
    private array $items = [30];
}
class ForeignOwner {
    private $value = 'hidden';
    public function __get($name) { return 'magic:' . $name; }
}
$parent = new ParentOwner;
$child = new ChildOwner;
$inherited = new InheritedOwner;
$recomposed = new RecomposedOwner;
$foreign = new ForeignOwner;
// The target class is identical while the executing trait consumer changes.
for ($i = 0; $i < 4; $i++) {
    echo $parent->peer($child), '|', $child->peer($child), '|';
    echo $inherited->peer($child), '|', $recomposed->own(), '|';
    echo $parent->peer($foreign), "\n";
    $parent->push(); $child->push();
}
$snapshot = $parent->items();
$snapshot[] = 99;
echo json_encode($parent->items()), '|', json_encode($child->items()), '|', json_encode($snapshot), "\n";
// A callable closure enters with an explicit lexical scope on the same body.
$callable = Closure::fromCallable([$parent, 'peer']);
for ($i = 0; $i < 3; $i++) {
    echo $callable($child), '|', $child->peer($child), '|', $parent->peer($child), "\n";
}
$closure = function () { return $this->value; };
$boundParent = Closure::bind($closure, $child, ParentOwner::class);
$boundChild = Closure::bind($closure, $child, ChildOwner::class);
for ($i = 0; $i < 3; $i++) { echo $boundParent(), '|', $boundChild(), "\n"; }
