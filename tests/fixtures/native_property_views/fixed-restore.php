<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "diagnostic:$level:$message\n"; return true; });
function attempt($label, $call) {
    echo "case:$label\n";
    try { var_dump($call()); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
$cell = 11;
$input = [&$cell, 'note' => 'saved'];
$target = new SplFixedArray;
attempt('populate', fn() => $target->__unserialize($input));
$cell = 22;
$input[1] = 33;
var_dump($target->toArray(), (array) $target);
attempt('repeat', fn() => $target->__unserialize([5, 'note' => 'ignored']));
var_dump($target->toArray(), (array) $target);
unset($target[0]);
var_dump($target->toArray(), $cell);
foreach ([[-1 => 'negative'], [3 => 'sparse'], ['x' => 'only-member'], ['00' => 'leading-zero']] as $case) {
    $receiver = new SplFixedArray;
    attempt('keys', fn() => $receiver->__unserialize($case));
    var_dump($receiver->toArray(), (array) $receiver);
}
attempt('wrong-input', fn() => $target->__unserialize(null));
attempt('too-many', fn() => $target->__serialize(1));
$existing = new SplFixedArray(2);
$existing[0] = 'kept';
attempt('initialized', fn() => $existing->__unserialize(['new']));
var_dump($existing->toArray());
class TypedSlots extends SplFixedArray { public int $count = 2; }
$typed = new TypedSlots;
attempt('typed-member', fn() => $typed->__unserialize([8, 'count' => 'wrong']));
var_dump($typed->toArray(), $typed->count);
set_error_handler(function ($level, $message) { echo "throwing:$level\n"; throw new Exception('stopped'); });
$stopped = new SplFixedArray;
attempt('member-error', fn() => $stopped->__unserialize([9, 'member' => 4, 10]));
var_dump($stopped->toArray(), (array) $stopped);
