<?php

class InstancePropertyConstructor
{
    public function __construct(public $value) {}
}

$box = new InstancePropertyConstructor(0);
$start = microtime(true);
for ($i = 0; $i < 1000000; $i++) {
    $box = new InstancePropertyConstructor($i);
}
$elapsed = microtime(true) - $start;
echo $box->value . '|' . $elapsed;
