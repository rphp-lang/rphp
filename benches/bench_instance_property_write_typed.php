<?php

class TypedInstancePropertyWrite
{
    public int $value = 0;
}

$box = new TypedInstancePropertyWrite();
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $box->value = $i;
}
$elapsed = microtime(true) - $start;
echo $box->value . '|' . $elapsed;
