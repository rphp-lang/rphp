<?php
function numericForward($left, $right, $initial) {
    $value = $initial;
    for ($index = 0; $index < 9; ++$index) {
        $value = $left + $right;
    }
    return $value;
}
foreach ([null, false, true, -4, 0.5] as $initial) {
    $integer = numericForward(17, -3, $initial);
    $fraction = numericForward(0.25, 0.5, $initial);
    $overflow = numericForward(PHP_INT_MAX, 1, $initial);
    $zero = numericForward(-0.0, -0.0, $initial);
    $infinity = numericForward(INF, 2.0, $initial);
    $nan = numericForward(INF, -INF, $initial);
    echo gettype($integer), ':', $integer, ':', $fraction, ':',
        gettype($overflow), ':', bin2hex(pack('E', $zero)), ':',
        (int)is_infinite($infinity), ':', (int)is_nan($nan), "\n";
}
$value = 3;
$alias =& $value;
$value = 1.5 + 2.5;
echo $value, ':', $alias, "\n";
class NumericRetirement {
    public function __destruct() { echo "retire\n"; }
}
$value = new NumericRetirement;
$value = 2 + 7;
echo $value, "\n";
function numericLeaf($value) { return $value + 7; }
function numericZero() { return 9; }
echo numericLeaf(2), ':', numericZero(), "\n";
try {
    numericLeaf();
} catch (ArgumentCountError $error) {
    echo "missing\n";
}
$sideEffect = 0;
echo numericLeaf(3, ++$sideEffect), ':', $sideEffect, ':',
    numericLeaf(value: 4), ':', numericZero(23), "\n";
$argument = 5;
$argumentAlias =& $argument;
echo numericLeaf($argumentAlias), ':', $argument, "\n";

class NumericFrameSentinel {
    public static $retired = 0;
    public function __destruct() { ++self::$retired; }
}
// Exercise both sides of the physical 64-slot boundary, including surplus
// arguments that enlarge a compact declaration's actual activation.
foreach ([0, 40, 80] as $width) {
    $body = '';
    for ($slot = 0; $slot < $width; ++$slot) {
        $body .= '$local' . $slot . ' = null;';
    }
    $name = 'numericEnvelope' . $width;
    eval('function ' . $name . '() {' . $body . '
        $sum = 0;
        $held = null;
        for ($index = 0; $index < 6; ++$index) {
            $held = ($index % 2 === 0) ? new NumericFrameSentinel : $index + 1;
            $sum += is_object($held) ? numericLeaf($index) : $held;
            $less = $index < 4;
            $equal = $index === 3;
            $sum += (int)$less + (int)$equal;
        }
        return $sum;
    }');
    echo $name(), ':', $name(...array_fill(0, 80, null)), ':',
        NumericFrameSentinel::$retired, "\n";
}
function numericLateCaller($value) { return numericPublishedLater($value); }
try {
    numericLateCaller(1);
} catch (Error $error) {
    echo "unresolved\n";
}
eval('function numericPublishedLater($value) { return $value + 11; }');
echo numericLateCaller(1), ':', numericLateCaller(2), "\n";
function numericWarmCaller($value) { return numericLeaf($value); }
foreach ([null, false, 2.5, '3'] as $input) {
    echo numericWarmCaller($input), ':';
}
try {
    numericWarmCaller([]);
} catch (TypeError $error) {
    echo "type\n";
}

// Integer-kind primitives and reference views must use the same arithmetic
// result without changing either operand or its alias.
foreach ([null, false, true, -7, 8] as $left) {
    foreach ([null, false, true, -7, 8] as $right) {
        $leftAlias =& $left;
        $rightAlias =& $right;
        $before = serialize([$left, $right]);
        $sum = numericForward($leftAlias, $rightAlias, 1.25);
        echo gettype($sum), ':', $sum, ':',
            (int)($before === serialize([$leftAlias, $rightAlias])), ';';
        unset($leftAlias, $rightAlias);
    }
    echo "\n";
}
