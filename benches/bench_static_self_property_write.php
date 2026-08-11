<?php

class StaticSelfPropertyWriteBench
{
    public static $value = 0;

    public static function run(int $iterations): int
    {
        for ($i = 1; $i <= $iterations; $i++) {
            self::$value = $i;
        }
        return self::$value;
    }
}

class StaticSelfPropertyWriteBenchChild extends StaticSelfPropertyWriteBench
{
    public static $value = 0;
}

$start = microtime(true);
$value = StaticSelfPropertyWriteBenchChild::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
