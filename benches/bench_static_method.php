<?php

// Ordinary static-call control. In particular, this must not pay for the
// sparse late-static return sidecar when the signature does not use `static`.
class StaticCalculatorControl
{
    public static function step($value, $delta)
    {
        return (($value * 3) + $delta) % 1000003;
    }
}

$start = microtime(true);
$value = 7;
for ($i = 0; $i < 20000000; $i++) {
    $value = StaticCalculatorControl::step($value, $i);
}
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
