<?php

trait DispatchTrait {
    public function traitValue() { return 3; }
}
class DispatchRoot {
    protected static function factor() { return 7; }
    public function baseValue() { return static::factor(); }
}
class DispatchA extends DispatchRoot { use DispatchTrait; }
class DispatchB extends DispatchRoot { use DispatchTrait; }
class DispatchC extends DispatchRoot { use DispatchTrait; }
class DispatchD extends DispatchRoot { use DispatchTrait; }
class DispatchReader {
    private function secret() { return 0; }
    public function read($object) { return $object->baseValue() + $object->traitValue() + $this->secret(); }
}
$pool = [new DispatchA, new DispatchB, new DispatchC, new DispatchD];
$reader = new DispatchReader;
$checksum = 0;
$startedAt = microtime(true);
for ($index = 0; $index < 100_000; $index++) {
    $checksum += $reader->read($pool[$index & 3]);
}
$elapsed = microtime(true) - $startedAt;
echo $checksum . '|' . $elapsed;
