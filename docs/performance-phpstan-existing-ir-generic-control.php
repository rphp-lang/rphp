<?php
function project($a) {
    $first = $a['x'];
    $second = $a['y'];
    $total = $first + $second;
    return $total;
}
function project_branch($a) {
    $first = $a['x'];
    $second = $a['y'];
    if ($first < $second) { $first += $second; }
    return $first;
}
function project_then_effect($a, &$effects) {
    $first = $a['x'];
    $second = $a['y'];
    $total = $first + $second;
    $effects[] = $total;
    return $total;
}
$sum = 0;
for ($i = 0; $i < 300; ++$i) {
    $sum += project(['x' => 3, 'y' => 5]);
    $sum += project_branch(['x' => 3, 'y' => 5]);
    $sum += project_branch(['x' => 7, 'y' => 2]);
}
var_dump($sum);
foreach ([['x' => '3', 'y' => 5], ['x' => 1.5, 'y' => 5], ['x' => PHP_INT_MAX, 'y' => 1]] as $a) {
    var_dump(project($a), project_branch($a));
}
$x = 3;
$a = ['x' => &$x, 'y' => 5];
var_dump(project($a), $x);
$effects = [];
for ($i = 0; $i < 100; ++$i) { project_then_effect(['x' => 3, 'y' => 5], $effects); }
var_dump(count($effects), $effects[0]);
set_error_handler(function ($severity, $message) { echo "warning\n"; return true; });
var_dump(project(['y' => 5]));
try { project_then_effect(['x' => [], 'y' => 5], $effects); }
catch (TypeError $e) { echo "type-error\n"; }
var_dump(count($effects));
