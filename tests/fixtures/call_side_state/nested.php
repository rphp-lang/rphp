<?php
function scalarArgument($label, int $value = 3): int {
    echo $label, ':', strlen('abc'), ':', count([1, 2]), '|';
    return $value;
}
class ArgumentReceiver {
    function __construct(public string $name) {}
    function __invoke($left, $right = 1) {
        echo $this->name, ':', $left, ':', $right, '|';
        return $left + $right;
    }
}
class MagicReceiver {
    function __call($name, $arguments) {
        echo $name, ':', implode(',', $arguments), '|';
        return count($arguments);
    }
}
$receiver = new ArgumentReceiver('invoke');
$magic = new MagicReceiver;
echo $receiver(scalarArgument('left'), $receiver(scalarArgument('inner'), 2)), '|';
echo $magic->missing(scalarArgument('magic'), $receiver(scalarArgument('nested'), 4)), '|';
try {
    $receiver(scalarArgument('before'), scalarArgument('invalid', []));
} catch (TypeError $error) {
    echo 'type:', $error->getTrace()[0]['function'], '|';
}
echo $receiver(right: scalarArgument('named-right'), left: scalarArgument('named-left')), '|done';
