<?php
function copyRegistered($source) { return clone $source; }
class RegisteredClone extends stdClass {
    function __clone() {
        $temporary = copyRegistered((object) ['n' => 10]);
        $this->n += $temporary->n;
    }
}
class DerivedClone extends RegisteredClone {}
$hook = new DerivedClone;
$hook->n = 3;
$items = [(object) ['n' => 1], json_decode('{"n":2}'), $hook,
          unserialize('O:8:"stdClass":1:{s:1:"n";i:4;}'), new stdClass];
foreach ($items as $source) {
    for ($i = 0; $i < 2; ++$i) {
        $copy = copyRegistered($source);
        $copy->extra = 9;
        echo get_class($copy), ':', $source->n ?? 0, ':', $copy->n ?? 0,
             ':', isset($source->extra) ? 1 : 0, ':', $source === $copy ? 1 : 0, "\n";
    }
}
