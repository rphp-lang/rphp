<?php
function capture_check($actual, $expected) {
    if ($actual !== $expected) { throw new Exception('capture mismatch'); }
}
function capture_sum($left, $right) { return $left + $right; }
function capture_difference($left, $right) { return $right - $left; }
function capture_square($value) { return $value * $value; }
function capture_remainder($left, $right) { return $left % $right; }
function capture_xor($left, $right) { return $left ^ $right; }
function capture_compare($left, $right) { return $left <=> $right; }
function capture_eighth($a, $b, $c, $d, $e, $f, $g, $h) { return $h + $a; }
function capture_unused(int $used, int $unused) { return $used + 3; }
function capture_multi($a, $b) { return ($a + 1) * ($b - 1); }
function capture_select($a, $b) { if ($a < $b) { return $a + 1; } return $b - 1; }
class CaptureMethod {
    function difference($left, $right) { return $right - $left; }
    static function sum($left, $right) { return $left + $right; }
}
$object = new CaptureMethod;
for ($i = 0; $i < 40; $i++) {
    capture_check(capture_sum($i, 4), $i + 4);
    capture_check(capture_difference(2, $i), $i - 2);
    capture_check(capture_square($i), $i * $i);
    capture_check(capture_remainder($i, 3), $i % 3);
    capture_check(capture_xor($i, 11), $i ^ 11);
    capture_check(capture_compare($i, 20), $i <=> 20);
    capture_check(capture_eighth(1,2,3,4,5,6,7,$i), $i + 1);
    capture_check($object->difference($i, 5), 5 - $i);
    capture_check(CaptureMethod::sum($i, 2), $i + 2);
    capture_check(capture_multi($i, 4), ($i + 1) * 3);
    capture_check(capture_select($i, 20), $i < 20 ? $i + 1 : 19);
}
capture_check(gettype(capture_sum(PHP_INT_MAX, 1)), 'double');
capture_check(gettype(capture_square(PHP_INT_MAX)), 'double');
capture_check(capture_sum(1.5, 2), 3.5);
capture_check(capture_sum('7', 2), 9);
capture_check(capture_sum(right: 8, left: 2), 10);
capture_check(capture_difference(...[2, 7]), 5);
capture_check(capture_sum(1, 2, 3), 3);
$value = 9; $alias =& $value;
capture_check(capture_sum($alias, 3), 12);
capture_check($alias, 9);
capture_sum($alias, 1); // Unused result still advances exactly one call.
$trace = '';
function capture_replace(&$value) { global $trace; $trace .= 'r'; $value = 30; return 2; }
capture_check(capture_sum($value, capture_replace($value)), 11);
capture_check($trace, 'r'); capture_check($value, 30);
function capture_throw(&$trace) { $trace .= 't'; throw new Exception('argument'); }
try { capture_sum(1, capture_throw($trace)); } catch (Exception $error) {
    capture_check($error->getMessage(), 'argument');
}
capture_check($trace, 'rt');
$caught = 0;
try { capture_sum([], 1); } catch (TypeError $error) { $caught++; }
try { capture_unused(2, []); } catch (TypeError $error) { $caught++; }
try { capture_sum(1); } catch (ArgumentCountError $error) { $caught++; }
try { capture_remainder(1, 0); } catch (DivisionByZeroError $error) { $caught++; }
capture_check($caught, 4);
$warnings = 0;
set_error_handler(function ($severity, $message) use (&$warnings) { $warnings++; return true; });
capture_check(capture_sum($notDefined, 2), 2);
restore_error_handler();
capture_check($warnings, 1);
capture_check(capture_sum(20, 22), 42);
echo "capture:ok\n";
