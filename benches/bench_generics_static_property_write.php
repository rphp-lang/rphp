<?php

class StaticGenericPropertyValue<T> {}

class StaticGenericPropertyWriteBench
{
    public static StaticGenericPropertyValue<int> $value;

    public static function run(int $iterations, StaticGenericPropertyValue $value): int
    {
        for ($i = 1; $i <= $iterations; $i++) {
            self::$value = $value;
        }
        return $iterations;
    }
}

$value = new StaticGenericPropertyValue::<int>();
$start = microtime(true);
$result = StaticGenericPropertyWriteBench::run(5000000, $value);
$elapsed = microtime(true) - $start;

echo $result . '|' . $elapsed;
