<?php

// New RFC call form. Compare this candidate-only workload with the explicit
// owner control below; the exact baseline intentionally rejects self::<...>.
class GenericStaticSelfBench
{
    public static function step<T : int>(T $value): T
    {
        return $value + 1;
    }

    public static function run(int $iterations): int
    {
        $value = 0;
        for ($i = 0; $i < $iterations; $i++) {
            $value = self::step::<int>($value);
        }
        return $value;
    }
}

$start = microtime(true);
$value = GenericStaticSelfBench::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
