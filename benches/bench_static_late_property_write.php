<?php

class StaticLatePropertyWriteBench
{
    public static $value = 0;

    public static function run(int $iterations): int
    {
        for ($i = 1; $i <= $iterations; $i++) {
            static::$value = $i;
        }
        return static::$value;
    }
}

class StaticLatePropertyWriteBenchChild extends StaticLatePropertyWriteBench
{
    public static $value = 0;
}

$start = microtime(true);
$value = StaticLatePropertyWriteBenchChild::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
