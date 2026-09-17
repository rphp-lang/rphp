<?php
class TypedCells implements IteratorAggregate {
    public int $first = 17;
    public int $second = 29;
    public function &getIterator(): Traversable {
        yield $this->first;
        yield $this->second;
    }
}
$object = new TypedCells;
foreach ($object as &$slot) { $slot += 2; }
try { $slot = []; } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
echo $object->first, ':', $object->second, ':', $slot, "\n";
$alias =& $slot;
unset($object, $slot);
$alias = 50;
echo $alias, "\n";
