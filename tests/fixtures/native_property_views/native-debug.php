<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "diagnostic:$level:$message\n"; return true; });
class SelfView extends ArrayObject {
    public $label = 'self';
    public function connect() { $this->exchangeArray($this); }
}
$self = new SelfView;
$self->connect();
echo "self-debug\n";
var_dump($self->__debugInfo(), $self);
class DebugView extends ArrayObject {
    public function __debugInfo(): array { return ['slots' => count(parent::__debugInfo()), 'visible' => 7]; }
}
echo "override-debug\n";
$debug = new DebugView(['hidden' => 19]);
var_dump($debug);
class IteratorView extends ArrayIterator {
    public $name = 'cursor';
    public function __debugInfo(): array { $out = parent::__debugInfo(); return ['members' => count($out), 'native' => count($out["\0ArrayIterator\0storage"])]; }
}
var_dump(new IteratorView([3, 4]));
$plain = new ArrayObject(['kept' => 2]);
$snapshot = $plain->__debugInfo();
$snapshot["\0ArrayObject\0storage"]['kept'] = 9;
var_dump($plain->getArrayCopy());
