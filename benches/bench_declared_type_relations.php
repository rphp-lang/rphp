<?php
interface RelationRoot {}
interface RelationLeaf extends RelationRoot {}
class RelationBase implements RelationLeaf {}
class RelationChild extends RelationBase {}
class RelationOther {}
function relations($value): int {
    return (int) ($value instanceof RelationRoot)
        + (int) ($value instanceof RelationBase)
        + (int) ($value instanceof RelationChild)
        + (int) ($value instanceof RelationOther);
}
$values = [new RelationChild, new RelationBase, new RelationOther, new stdClass, null, 17, function () {}, new class extends RelationChild {}];
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < 500_000; $i++) $sum += relations($values[($i >> 2) & 7]);
echo $sum, '|', microtime(true) - $start;
