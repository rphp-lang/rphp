<?php

class GenericMethodParent<T>
{
    public function step(T $value): T
    {
        return $value + 1;
    }
}

class ConcreteIntMethodBox extends GenericMethodParent<int>
{
}

$box = new ConcreteIntMethodBox();
$start = microtime(true);
$value = 0;
for ($i = 0; $i < 5000000; $i++) {
    $value = $box->step($value);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
