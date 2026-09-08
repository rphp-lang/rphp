<?php
class MovedNote {
    public function __construct(public string $label) {}
    public function __destruct() { echo 'drop:', $this->label, "\n"; }
}
class MoveFactory {
    protected static string $label = 'base';
    public static function make() { return new MovedNote(static::$label); }
}
class LeafFactory extends MoveFactory { protected static string $label = 'leaf'; }
function &borrow_value(&$owner) { return $owner; }
function exercise_moves() {
    for ($i = 0; $i < 3; $i++) {
        $object = LeafFactory::make();
        $values = [$object, $i];
        $copy = $values;
        $copy[1] = 30;
        $closure = static function () use ($object) { return $object->label; };
        echo $closure(), ':', $values[1], ':', $copy[1], "\n";
        unset($object, $values, $copy, $closure);
        $resource = fopen('php://memory', 'w+');
        fwrite($resource, 'r' . $i);
        rewind($resource);
        // Exercise the moved native resource even without the optional
        // stream registry. The payload written above is exactly two bytes.
        echo fread($resource, 2), "\n";
        fclose($resource);
        $owner = ['before'];
        $snapshot = borrow_value($owner);
        $snapshot[0] = 'after';
        echo $owner[0], ':', $snapshot[0], "\n";
    }
}
exercise_moves();
echo "done\n";
