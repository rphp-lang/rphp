<?php
class DynamicParent { protected function inherited() { return 'parent'; } }
class DynamicChild extends DynamicParent {
    public function callIt($name) { return $this->$name(); }
    public function __call($name, $args) { return 'magic:' . $name; }
}
$objects = [new DynamicChild, new DynamicChild];
foreach ($objects as $object) {
    foreach (['inherited', 'INHERITED', 'missing', 'MiSsInG'] as $name) {
        echo $name, ':', (int)method_exists($object, $name), ':', $object->callIt($name), "\n";
    }
}
eval('class DynamicGrandchild extends DynamicChild { public function later() { return "late"; } }');
$object = new DynamicGrandchild;
foreach (['inherited', 'LATER', 'missing'] as $name) {
    echo $name, ':', (int)method_exists($object, $name), ':', $object->callIt($name), "\n";
}
