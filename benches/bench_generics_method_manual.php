<?php

class ManualMethodBox
{
    public function step(int $value): int
    {
        return $value + 1;
    }
}

$box = new ManualMethodBox();
$start = microtime(true);
$value = 0;
for ($i = 0; $i < 5000000; $i++) {
    $value = $box->step($value);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
