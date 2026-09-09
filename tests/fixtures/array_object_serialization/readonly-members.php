<?php
class RestoredReadonly extends ArrayObject {
    public readonly int $number;
    public string $before = 'old';
    public string $after = 'old';
    public function __construct($initialize) {
        parent::__construct(['old' => 151]);
        if ($initialize) $this->number = 3;
    }
}
class RestoredTypedMember extends ArrayIterator { public int $number = 7; }
foreach (['modern', 'legacy'] as $mode) {
    $object = new RestoredTypedMember;
    $cell = 'wrong';
    $members = ['number' => &$cell];
    if ($mode === 'modern') $object->__unserialize([0, [], $members]);
    else $object->unserialize('x:i:0;a:0:{};m:'.serialize($members));
    echo $mode, ':', gettype($object->number), "\n";
    $cell = [];
    echo gettype($object->number), "\n";
    attempt(function () use ($object) { $object->number = 'still-wrong'; });
}
foreach (['modern', 'legacy'] as $mode) {
    foreach ([false, true] as $initialize) {
        foreach ([5, 'wrong'] as $number) {
            $object = new RestoredReadonly($initialize);
            $members = ['before' => 'changed', 'number' => $number, 'after' => 'changed'];
            echo $mode, ':', (int)$initialize, ':', gettype($number), "\n";
            attempt(function () use ($mode, $object, $members) {
                if ($mode === 'modern') $object->__unserialize([0, ['fresh' => 157], $members]);
                else $object->unserialize('x:i:0;'.serialize(['fresh' => 157]).';m:'.serialize($members));
                echo "restored\n";
            });
            echo json_encode($object->getArrayCopy()), ':', $object->before, ':', $object->after, ':', isset($object->number) ? $object->number : 'unset', "\n";
        }
    }
}
