<?php
function combineValues($a, $b) { return $a + $b; }
function maskValues($a, $b) { return $a ^ $b; }
function remainderValues($a, $b) { return $a % $b; }
function chooseValue($a, $b) { return $a < 0 ? $a + $b : $a - $b; }
echo combineValues(7, 12), ':', combineValues('6', 9), ':', combineValues(2.5, 4), "\n";
echo gettype(combineValues(PHP_INT_MAX, 1)), ':', maskValues(9, 3), ':', bin2hex(maskValues('A', 'B')), "\n";
foreach ([0, 2] as $divisor) {
    try { echo remainderValues(17, $divisor), "\n"; }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
try { combineValues([], 2); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
$original = 14;
$alias =& $original;
echo combineValues($alias, 3), ':', $original, ':', chooseValue(-5, 7), ':', chooseValue(5, 7), "\n";
function nextOperand() { static $value = 0; echo ++$value; return $value; }
echo ':', combineValues(nextOperand(), nextOperand()), "\n";
