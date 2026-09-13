<?php
class SlotReuseOwner {
    public static int $released = 0;
    public function __construct(public int $value) {}
    public function __destruct() { self::$released++; }
}
function exerciseSlotReuse() {
    // FRAME_LOCALS
    $kept = [];
    $sum = 0;
    for ($i = 0; $i < 64; $i++) {
        $object = new SlotReuseOwner($i);
        $sum += $object->value;
        if ($i % 7 === 0) { $kept[] = $object; }
        unset($object);
    }
    echo $sum, ':', SlotReuseOwner::$released, ':';
    unset($kept);
    echo SlotReuseOwner::$released, "\n";
}
exerciseSlotReuse();
