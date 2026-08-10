<?php

class ManualDiamondBox
{
    public function step(int|float $value): int
    {
        return $value + 1;
    }
}

$box = new ManualDiamondBox();
$start = microtime(true);
$value = 0;
for ($i = 0; $i < 5000000; $i++) {
    $value = $box->step($value);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
