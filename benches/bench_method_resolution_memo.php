<?php
interface MemoBenchValue { public function read(int $n): int; }
class MemoBenchA implements MemoBenchValue { public $value = 2; public function read(int $n): int { return $this->value + $n; } }
class MemoBenchB implements MemoBenchValue { public $value = 5; public function read(int $n): int { return $this->value + $n; } }
class MemoBenchC implements MemoBenchValue { public $value = 7; public function read(int $n): int { return $this->value + $n; } }
class MemoBenchD implements MemoBenchValue { public $value = 11; public function read(int $n): int { return $this->value + $n; } }
class MemoBenchDispatch { public function read(MemoBenchValue $receiver, int $n): int { return $receiver->read($n); } }
$values = [new MemoBenchA, new MemoBenchB, new MemoBenchC, new MemoBenchD];
$dispatch = new MemoBenchDispatch;
$checksum = 0;
$start = microtime(true);
for ($i = 0; $i < 500_000; $i++) $checksum += $dispatch->read($values[$i & 3], $i & 15);
echo $checksum, '|', microtime(true) - $start, "\n";
