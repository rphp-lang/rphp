<?php
declare(strict_types=1);
function strictFloatProjection(float $value) { return $value; }
function strictUnionProjection(int|float $value) { return $value; }
$input = 17;
$closure = fn(float $value) => $value;
var_dump(strictFloatProjection($input), $closure($input), $input);
var_dump(strictFloatProjection(2.5), strictUnionProjection($input));
try { strictFloatProjection('17'); } catch (TypeError $error) { echo "string rejected\n"; }
echo "done\n";
