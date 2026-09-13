<?php
class BaseWire {
    public int $number = 13;
}
enum TokenWire { case Live; }
class_alias(BaseWire::class, 'AliasWire');
$events = [];
spl_autoload_register(function ($name) use (&$events) {
    $events[] = $name;
    if ($name === 'LateWire') {
        class LateWire extends BaseWire {}
    } elseif ($name === 'ForbiddenWire') {
        class_alias(SplFileInfo::class, $name);
    } elseif ($name === 'ExceptionWire') {
        throw new RuntimeException('autoload stopped');
    }
});
foreach (['BaseWire', 'aLIASwIRE', 'LateWire'] as $name) {
    $object = unserialize('O:' . strlen($name) . ':"' . $name . '":1:{s:6:"number";i:29;}');
    echo get_class($object), ':', $object->number, "\n";
}
foreach (['ForbiddenWire', 'ExceptionWire'] as $name) {
    try {
        unserialize('O:' . strlen($name) . ':"' . $name . '":0:{}');
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
$missing = unserialize('O:11:"MissingWire":1:{s:6:"number";i:7;}');
echo serialize($missing), "\n";
$leaf = (object)['number' => 17];
$copy = unserialize(serialize([$leaf, $leaf, TokenWire::Live, TokenWire::Live]));
echo json_encode([$copy[0] === $copy[1], $copy[0]->number,
    $copy[2] === TokenWire::Live, $copy[2] === $copy[3], $events]), "\n";
