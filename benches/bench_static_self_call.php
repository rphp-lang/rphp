<?php

class StaticSelfBench
{
    public static function step(int $value): int
    {
        return $value + 1;
    }

    public static function run(int $iterations): int
    {
        $value = 0;
        for ($i = 0; $i < $iterations; $i++) {
            $value = self::step($value);
        }
        return $value;
    }
}

class StaticSelfBenchChild extends StaticSelfBench
{
    public static function step(int $value): int
    {
        return $value + 1;
    }
}

$start = microtime(true);
$value = StaticSelfBenchChild::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
