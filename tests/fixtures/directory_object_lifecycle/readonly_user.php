<?php
class ReadonlyReferenceForSpec {
    public readonly int $implicit;
    public protected(set) readonly int $explicit;
    protected readonly int $hidden;
    public private(set) int $narrow;
    public function __construct() { $this->implicit = 1; $this->hidden = 2; }
}
$object = new ReadonlyReferenceForSpec;
foreach (['implicit', 'explicit', 'hidden'] as $name) {
    try { $reference =& $object->$name; }
    catch (Throwable $e) { echo $e->getMessage(), "\n"; }
}
foreach ((new ReflectionClass(ReadonlyReferenceForSpec::class))->getProperties() as $property) {
    echo $property->getName(), ':', $property->getModifiers(), "\n";
}
var_dump($object->implicit, isset($object->explicit));
