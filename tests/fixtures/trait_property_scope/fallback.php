<?php
trait ScopedFallbacks {
    public function label() { return __CLASS__ . ':' . $this->value; }
    public static function staticRead($object) { return $object->value; }
    public function readNumber() { return $this->number; }
    public function clearNumber() { unset($this->number); }
    public function pushReadonly() { $this->items[] = 2; }
}
class FirstFallback {
    use ScopedFallbacks;
    private string $value = 'first';
    private int $number = 7;
    public function __construct(private readonly array $items = [1]) {}
    public function __get($name) { return 42; }
}
class SecondFallback {
    use ScopedFallbacks;
    private string $value = 'second';
    private int $number = 8;
    public function __construct(private readonly array $items = [3]) {}
}
$first = new FirstFallback;
$second = new SecondFallback;
for ($i = 0; $i < 3; $i++) {
    echo $first->label(), '|', $second->label(), '|';
    echo FirstFallback::staticRead($first), '|', SecondFallback::staticRead($second), '|';
    echo $first->readNumber(), '|', $second->readNumber(), "\n";
    try { $first->pushReadonly(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
}
$first->clearNumber(); $second->clearNumber();
for ($i = 0; $i < 2; $i++) {
    echo $first->readNumber(), '|';
    try { echo $second->readNumber(); } catch (Error $error) { echo $error->getMessage(); }
    echo "\n";
}
