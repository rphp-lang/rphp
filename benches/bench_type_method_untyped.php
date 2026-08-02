<?php

class TypedCalculator
{
    public function step($value, $delta)
    {
        return (($value * 3) + $delta) % 1000003;
    }
}

$calculator = new TypedCalculator();
$start = microtime(true);
$value = 7;
for ($i = 0; $i < 5000000; $i++) {
    $value = $calculator->step($value, $i);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
