<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "diagnostic:$level:$message\n"; return true; });
class PrintedView {
    public $hidden = 17;
    public function __debugInfo(): array { echo "hook\n"; return ['shown' => 23, 'loop' => $this]; }
}
$object = new PrintedView;
echo "root\n";
print_r($object);
echo "nested\n";
print_r(['box' => $object]);
echo "returned\n";
$text = print_r($object, true);
echo $text;
echo "deque\n";
$deque = new SplDoublyLinkedList;
$deque->push('one');
$deque->push('two');
print_r($deque);
