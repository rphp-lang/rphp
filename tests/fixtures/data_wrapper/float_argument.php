<?php
function floatProjection(float $value) { return $value; }
function nullableProjection(?float $value) { return $value; }
function unionProjection(int|float $value) { return $value; }
function referenceProjection(float &$value) { return $value; }
class FloatProjectionReceiver {
    public function project(float $value) { return $value; }
}
$closure = function (float $value) { return $value; };
$arrow = fn(float $value) => $value;
$receiver = new FloatProjectionReceiver();
$input = 17;
var_dump(floatProjection($input), $input);
foreach (['floatProjection', $closure, $arrow, [$receiver, 'project']] as $call) {
    foreach ([17, -3, 2.5] as $value) {
        var_dump($call($value), $value);
    }
}
var_dump(nullableProjection(17), nullableProjection(null));
var_dump(unionProjection(17), unionProjection(2.5));
$alias =& $input;
var_dump(referenceProjection($input), $input, $alias);
try { floatProjection([]); } catch (TypeError $error) { echo "array rejected\n"; }
echo "done\n";
