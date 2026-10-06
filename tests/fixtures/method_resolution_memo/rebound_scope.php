<?php
class MemoScopedOwner {
    private function token($n) { return 'owner:' . $n; }
    function callback() { return function($receiver) { return $receiver->token(print 'arg|'); }; }
}
class MemoScopedLeft extends MemoScopedOwner {}
class MemoScopedRight extends MemoScopedOwner {}
class MemoUnrelated {}
$left = new MemoScopedLeft;
$right = new MemoScopedRight;
$callback = $left->callback();
echo $callback($left), '|', $callback($right), '|';
$denied = $callback->bindTo(new MemoUnrelated, MemoUnrelated::class);
try { echo $denied($left), '|'; } catch (Error $error) { echo 'denied|'; }
echo $callback($left), '|';
