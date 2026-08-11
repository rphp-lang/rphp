<?php

class StaticSelfPropertyBench
{
    public static $step = 1;

    public static function run(int $iterations): int
    {
        $value = 0;
        for ($i = 0; $i < $iterations; $i++) {
            $value = $value + self::$step;
        }
        return $value;
    }
}

class StaticSelfPropertyBenchChild extends StaticSelfPropertyBench
{
    public static $step = 1;
}

$start = microtime(true);
$value = StaticSelfPropertyBenchChild::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
