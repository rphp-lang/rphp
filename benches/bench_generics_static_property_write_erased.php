<?php

class StaticErasedPropertyValue<T> {}

class StaticErasedPropertyWriteBench
{
    public static StaticErasedPropertyValue $value;

    public static function run(int $iterations, StaticErasedPropertyValue $value): int
    {
        for ($i = 1; $i <= $iterations; $i++) {
            self::$value = $value;
        }
        return $iterations;
    }
}

$value = new StaticErasedPropertyValue::<int>();
$start = microtime(true);
$result = StaticErasedPropertyWriteBench::run(5000000, $value);
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
