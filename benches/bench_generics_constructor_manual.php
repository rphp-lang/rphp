<?php

class ManualConstructorBox
{
    public int $value;

    public function __construct(int $value)
    {
        $this->value = $value;
    }
}

$start = microtime(true);
$box = null;
for ($i = 0; $i < 1000000; $i++) {
    $box = new ManualConstructorBox($i);
}
$elapsed = microtime(true) - $start;

echo $box->value . '|' . $elapsed;
