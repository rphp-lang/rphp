<?php
class MemoRefLeft { public $cell = 3; function &read() { return $this->cell; } }
class MemoRefRight { public $cell = 7; function &read() { return $this->cell; } }
class MemoReferenceDispatch {
    function update($receiver, $number) { $alias =& $receiver->read(); $alias += $number; return $alias; }
}
$left = new MemoRefLeft;
$right = new MemoRefRight;
$dispatch = new MemoReferenceDispatch;
foreach ([$left, $right, $left, $right] as $n => $receiver) echo $dispatch->update($receiver, $n + 1), '|';
echo $left->cell, ':', $right->cell, '|';
