<?php

class InstancePropertyRead
{
    public $value = 7;
}

$box = new InstancePropertyRead();
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $sum += $box->value;
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
