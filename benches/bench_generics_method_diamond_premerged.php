<?php

trait PremergedDiamondStep<T>
{
    public function step(T $value): int
    {
        return $value + 1;
    }
}

class PremergedDiamondBox
{
    use PremergedDiamondStep<int|float>;
}

$box = new PremergedDiamondBox();
$start = microtime(true);
$value = 0;
for ($i = 0; $i < 5000000; $i++) {
    $value = $box->step($value);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
