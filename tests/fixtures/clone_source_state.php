<?php
class LazyCloneSource {
    public int $number = 2;
    public function __clone() { $this->number += 10; }
}
function cloneSource($value) { return clone $value; }
$reflection = new ReflectionClass(LazyCloneSource::class);
$plain = new LazyCloneSource();
$ghost = $reflection->newLazyGhost(function ($object) {
    echo "ghost\n";
    $object->number = 7;
});
$proxy = $reflection->newLazyProxy(function () {
    echo "proxy\n";
    $object = new LazyCloneSource();
    $object->number = 9;
    return $object;
});
foreach ([$plain, $ghost, $proxy, $proxy, $plain] as $source) {
    $alias =& $source;
    $copy = cloneSource($alias);
    echo json_encode([$source === $copy, $source->number, $copy->number,
        $reflection->isUninitializedLazyObject($source)]), "\n";
    unset($alias);
}
$broken = $reflection->newLazyGhost(function () {
    echo "throw\n";
    throw new RuntimeException('initializer stopped');
});
try { cloneSource($broken); } catch (RuntimeException $error) { echo $error->getMessage(), "\n"; }
echo json_encode([$reflection->isUninitializedLazyObject($broken),
    cloneSource($plain)->number, $plain->number]), "\n";
$calls = 0;
$producer = function () use (&$calls) { $calls++; return (object)['number' => 23]; };
$copy = clone $producer();
echo $copy->number, ':', $calls, "\n";
