<?php

class TypedStaticSelfPropertyWriteBench
{
    public static int $value = 0;

    public static function run(int $iterations): int
    {
        for ($i = 1; $i <= $iterations; $i++) {
            self::$value = $i;
        }
        return self::$value;
    }
}

class TypedStaticSelfPropertyWriteBenchChild extends TypedStaticSelfPropertyWriteBench
{
    public static int $value = 0;
}

$start = microtime(true);
$value = TypedStaticSelfPropertyWriteBenchChild::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
