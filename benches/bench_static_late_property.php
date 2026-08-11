<?php

class StaticLatePropertyBench
{
    public static $step = 1;

    public static function run(int $iterations): int
    {
        $value = 0;
        for ($i = 0; $i < $iterations; $i++) {
            $value = $value + static::$step;
        }
        return $value;
    }
}

class StaticLatePropertyBenchChild extends StaticLatePropertyBench
{
    public static $step = 1;
}

$start = microtime(true);
$value = StaticLatePropertyBenchChild::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
