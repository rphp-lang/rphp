<?php
class ReferenceBag implements IteratorAggregate {
    public array $items = [7, 9];
    public function &getIterator(): Traversable {
        foreach ($this->items as &$entry) { yield $entry; }
    }
}
$bag = new ReferenceBag;
$copy = $bag->items;
foreach ($bag as &$cell) { $cell *= 2; }
$cell = 60;
echo json_encode([$bag->items, $copy]), "\n";
$bag->items[1] = 75;
echo $cell, "\n";
unset($cell);
echo json_encode($bag->items), "\n";
