<?php

// Exercise a genuinely inherited late-static target. The control uses the
// same declarations and loop shape but binds the base implementation via
// self::; both step bodies intentionally produce the same result.
class StaticLateBench
{
    public static function step(int $value): int
    {
        return $value + 1;
    }

    public static function run(int $iterations): int
    {
        $value = 0;
        for ($i = 0; $i < $iterations; $i++) {
            $value = static::step($value);
        }
        return $value;
    }
}

class StaticLateBenchChild extends StaticLateBench
{
    public static function step(int $value): int
    {
        return $value + 1;
    }
}

$start = microtime(true);
$value = StaticLateBenchChild::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
