<?php

class InstancePropertyWrite
{
    public $value = 0;
}

$box = new InstancePropertyWrite();
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $box->value = $i;
}
$elapsed = microtime(true) - $start;
echo $box->value . '|' . $elapsed;
