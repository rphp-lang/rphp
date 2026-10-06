<?php
function memoArgument() { echo 'arg|'; return 4; }
class MemoConcrete { function pick($n) { return 'concrete:' . $n; } }
class MemoStatic { static function pick($n) { return 'static:' . (int)isset($this) . ':' . $n; } }
class MemoMagic { function __call($name, $args) { return 'magic:' . $name . ':' . $args[0]; } }
class MemoMixedDispatch {
    function literal($receiver) { return $receiver->pick(memoArgument()); }
    function dynamic($receiver) { $name = 'pick'; return $receiver->$name(memoArgument()); }
}
$dispatch = new MemoMixedDispatch;
$receivers = [new MemoConcrete, new MemoStatic, new MemoMagic];
for ($i = 0; $i < 2; $i++) foreach ($receivers as $receiver) echo $dispatch->literal($receiver), '|';
foreach ([$receivers[0], $receivers[1], $receivers[0]] as $receiver) echo $dispatch->dynamic($receiver), '|';
