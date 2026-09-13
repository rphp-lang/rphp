<?php
function numericWriteback($left, $right) {
    // FRAME_LOCALS
    $sum = 0;
    for ($index = 0; $index < 19; ++$index) {
        $sum += $left;
        $sum += $right;
    }
    $observed = ($sum += 2);
    echo gettype($sum), ':', $sum, ':', $observed, "\n";
}
foreach ([[true, false], [null, null], ['2', '3'], [0.25, 1.5], [7, -2]] as $pair) {
    numericWriteback($pair[0], $pair[1]);
}
$sum = 5;
$alias =& $sum;
$sum += 1.5;
echo $sum, ':', $alias, "\n";
class NumericProperty { public int $value = 9; }
$owner = new NumericProperty;
$constrained =& $owner->value;
$constrained += 2;
echo $constrained, ':', $owner->value, "\n";
$largest = PHP_INT_MAX;
$largest += 1;
echo gettype($largest), ':', (int)($largest > PHP_INT_MAX - 1), "\n";
$signed = -0.0;
$signed += -0.0;
echo bin2hex(pack('d', $signed)), "\n";
$array = ['left' => 1];
$array += ['left' => 9, 'right' => 2];
echo json_encode($array), "\n";
$live = 1;
function replaceNumericTarget() { $GLOBALS['live'] = 12; return 3; }
$live += replaceNumericTarget();
echo $live, "\n";
$warnings = 0;
set_error_handler(function ($level, $message) use (&$warnings) {
    ++$warnings;
    throw new RuntimeException('numeric warning');
});
$keep = 8;
try { $keep += '2suffix'; } catch (RuntimeException $error) { echo $error->getMessage(), ':', $keep, "\n"; }
restore_error_handler();
try { $keep += new stdClass; } catch (TypeError $error) { echo 'invalid:', $keep, "\n"; }
echo 'warnings:', $warnings, "\n";
