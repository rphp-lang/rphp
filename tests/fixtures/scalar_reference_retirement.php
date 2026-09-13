<?php
class RetirementProbe {
    public function __construct(public string $name) {}
    public function __destruct() { echo 'drop:', $this->name, "\n"; }
}
function retirementFrame(&$number, $createObject) {
    // FRAME_LOCALS
    $alias =& $number;
    $stream = fopen('php://memory', 'w+');
    fwrite($stream, 'state');
    $result = strlen('xy');
    $result = fopen('php://memory', 'w+');
    $saved = $result;
    $result = strlen('abc');
    if ($createObject) {
        $object = new RetirementProbe('local');
        $objectAlias = $object;
    }
    ++$alias;
    echo $result, ':', (int)is_resource($saved), ':', $number, "\n";
    return $number;
}
$number = 4;
echo retirementFrame($number, false), "\n";
echo retirementFrame($number, true), "\n";
$object = new stdClass;
$object->name = 'caller';
function borrowedRetirement(&$object) {
    $alias =& $object;
    return strlen('ok');
}
echo borrowedRetirement($object), ':', $object->name, "\n";
echo "done\n";
