<?php
function detached($value, ...$rest) {
    $value = 41;
    foreach ($rest as &$item) $item = 51;
}
function attached(&$value, &...$rest) {
    $value = 42;
    foreach ($rest as &$item) $item = 52;
}
foreach (['detached', 'attached'] as $call) {
    $items = [3, 4, 5];
    $call($items[0], $items[1], $items[2]);
    echo json_encode($items), "\n";
    $items = [6, 7];
    $call(value: $items[0], tail: $items[1]);
    echo json_encode($items), "\n";
}
$valueOnly = function ($value) { $value = 61; };
$reference = function (&$value) { $value = 62; };
class CallScope {}
foreach ([$valueOnly, $reference] as $call) {
    $item = 8;
    $call->__invoke($item);
    echo $item, "\n";
}
$item = 9;
$valueOnly->call(new CallScope, $item);
echo $item, "\n";
class ArgumentSink {
    function __invoke($value) { $value = 71; }
    static function changed(&$value) { $value = 72; }
}
foreach ([new ArgumentSink, ['ArgumentSink', 'changed']] as $call) {
    $items = [10];
    $call(value: $items[0]);
    echo json_encode($items), "\n";
}
