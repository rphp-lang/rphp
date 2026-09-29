<?php
trait BasicMethods { public function fromTrait() { return 'trait'; } }
class BasicParent {
    private function secret() { return 'parent-private'; }
    protected static function staticBase() { return 'static-base'; }
    public function direct() { return 'parent-direct'; }
    public function callSecret() { return $this->secret(); }
}
class BasicChild extends BasicParent {
    use BasicMethods;
    public function direct() { return 'child-direct'; }
    public function callStatic() { return static::staticBase(); }
}
class_alias(BasicChild::class, 'BasicOtherName');
$objects = [new BasicParent, new BasicChild, new BasicOtherName];
foreach ($objects as $object) {
    echo get_class($object), ':', $object->direct(), ':', $object->callSecret(), "\n";
    foreach (['fromTrait', 'FROMTRAIT', 'secret', 'direct', 'staticBase', 'callSecret', 'callStatic', 'missing'] as $name) {
        echo $name, ':', (int)method_exists($object, $name), ':', (int)is_callable([$object, $name]), ';';
    }
    echo "\n";
    foreach (['fromTrait', 'DIRECT', 'callStatic'] as $name) {
        try { echo $name, '=', $object->$name(), ';'; }
        catch (Throwable $error) { echo $name, '=', get_class($error), ';'; }
    }
    echo "\n";
}
