<?php

class GenericStaticLateBench
{
    public static function step<T : int>(T $value): T
    {
        return $value + 1;
    }

    public static function run(int $iterations): int
    {
        $value = 0;
        for ($i = 0; $i < $iterations; $i++) {
            $value = static::step::<int>($value);
        }
        return $value;
    }
}

class GenericStaticLateBenchChild extends GenericStaticLateBench
{
    public static function step<T : int>(T $value): T
    {
        return $value + 1;
    }
}

$start = microtime(true);
$value = GenericStaticLateBenchChild::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
