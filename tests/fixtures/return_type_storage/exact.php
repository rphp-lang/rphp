<?php

class ExactReturnOwner {
    public $marker = 7;
    function __destruct() { echo 'drop|'; }
}
class ExactReturnFactory {
    public $owner;
    function __construct($owner) { $this->owner = $owner; }
    function exact(): ExactReturnOwner { return $this->owner; }
    function preferred(): ExactReturnOwner|string { return $this->owner; }
}
function exactReturnArray(array &$values): array { return $values; }
$owner = new ExactReturnOwner;
$weak = WeakReference::create($owner);
$factory = new ExactReturnFactory($owner);
$values = [$owner];
$alias =& $values;
$read = exactReturnArray($alias);
$a = $factory->exact();
$b = $factory->preferred();
unset($owner, $factory, $values, $alias);
echo $a === $b, ':', $read[0] === $a, ':', $weak->get()->marker, '|';
unset($a, $b);
echo 'array-live:', $weak->get()->marker, '|';
unset($read);
echo 'gone:', $weak->get() === null, '|';
