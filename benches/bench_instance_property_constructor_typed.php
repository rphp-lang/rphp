<?php

class TypedInstancePropertyConstructor
{
    public function __construct(public int $value) {}
}

$box = new TypedInstancePropertyConstructor(0);
$start = microtime(true);
for ($i = 0; $i < 1000000; $i++) {
    $box = new TypedInstancePropertyConstructor($i);
}
$elapsed = microtime(true) - $start;
echo $box->value . '|' . $elapsed;
