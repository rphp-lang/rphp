<?php

trait GenericDiamondStep<T>
{
    public function step(T $value): int
    {
        return $value + 1;
    }
}

class GenericDiamondBox
{
    use GenericDiamondStep<int>, GenericDiamondStep<float>;
}

$box = new GenericDiamondBox();
$start = microtime(true);
$value = 0;
for ($i = 0; $i < 5000000; $i++) {
    $value = $box->step($value);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
