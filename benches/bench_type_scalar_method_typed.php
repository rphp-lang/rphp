<?php

class TypedScalarCalculator
{
    public function calculate(int $value): int
    {
        return ($value * 2) + 1;
    }
}

$calculator = new TypedScalarCalculator();
$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 5000000; $i++) {
    $sum = $sum + $calculator->calculate($i);
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
