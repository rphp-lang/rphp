<?php
function exercise() {
class ReplacedTempLeaf {
    function __construct(public $name) {}
    function __destruct() { echo 'drop:', $this->name, '|'; }
}
function replaceTempRow($copy, &$slot) {
    $slot = [new ReplacedTempLeaf('new')];
    return count($copy);
}
$rows = ['row' => [new ReplacedTempLeaf('old')]];
echo replaceTempRow($rows['row'], $rows['row']), '|after|';
unset($rows);
echo 'done|';
}
exercise();
