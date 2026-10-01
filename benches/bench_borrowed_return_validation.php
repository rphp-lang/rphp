<?php
interface ReturnIdentity {}
final class ReturnValue implements ReturnIdentity { public $id = 11; }
function exactReturnValue(ReturnIdentity $value): ReturnIdentity { return $value; }
function nullableReturnValue(?ReturnIdentity $value): ?ReturnIdentity { return $value; }
function exactReturnValues(array $values): array { return $values; }
function exactReturnUnion(ReturnIdentity $value): ReturnIdentity|string { return $value; }
$value = new ReturnValue;
$values = [$value, 17];
$checksum = 0;
$start = microtime(true);
for ($i = 0; $i < 500_000; $i++) {
    $read = exactReturnValue($value);
    $nullable = nullableReturnValue(($i & 1) === 0 ? $value : null);
    $row = exactReturnValues($values);
    $union = exactReturnUnion($value);
    $checksum += $read->id + $row[1] + ($union === $value) + ($nullable !== null);
}
echo $checksum, '|', microtime(true) - $start, "\n";
