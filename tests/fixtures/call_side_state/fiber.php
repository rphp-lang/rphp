<?php
function nestedScalar($value): int { return strlen($value); }
class FiberReceiver {
    function __invoke($left, $right) { echo 'invoke:', $left, ':', $right, '|'; return $left + $right; }
}
$receiver = new FiberReceiver;
$fiber = new Fiber(function () use ($receiver) {
    echo $receiver(nestedScalar('abc'), Fiber::suspend('paused')), '|';
});
echo $fiber->start(), ':', nestedScalar('outside'), '|';
$fiber->resume(5);
echo (int) $fiber->isTerminated(), '|done';
