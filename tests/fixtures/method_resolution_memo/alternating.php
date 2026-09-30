<?php
interface MemoValue { function read(int $n): int; }
class MemoLeft implements MemoValue { public $value = 2; function read(int $n): int { return $this->value + $n; } }
class MemoRight implements MemoValue { public $value = 5; function read(int $n): int { return $this->value + $n; } }
class MemoDispatch { function read(MemoValue $receiver, int $n): int { return $receiver->read($n); } }
class_alias(MemoLeft::class, 'MemoAlias');
$dispatch = new MemoDispatch;
$receivers = [new MemoLeft, new MemoRight, new MemoAlias, new MemoRight];
$sum = 0;
for ($i = 0; $i < 24; $i++) { $sum += $dispatch->read($receivers[$i & 3], $i); }
echo $sum, '|';
