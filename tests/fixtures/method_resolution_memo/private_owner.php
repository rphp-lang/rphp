<?php
class MemoOwner {
    private function token($n) { return 'owner:' . $n; }
    function invoke($receiver, $n) { return $receiver->token($n); }
}
class MemoShadowLeft extends MemoOwner { public function token($n) { return 'left:' . $n; } }
class MemoShadowRight extends MemoOwner { public function token($n) { return 'right:' . $n; } }
$owner = new MemoOwner;
$left = new MemoShadowLeft;
$right = new MemoShadowRight;
foreach ([$left, $right, $left, $right] as $n => $receiver) echo $owner->invoke($receiver, $n), '|';
echo $left->token(4), '|', $right->token(5), '|';
