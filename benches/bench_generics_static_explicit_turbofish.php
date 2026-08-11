<?php

// Same declaration, loop and call contract as the self:: control. Only the
// statically spelled owner differs, isolating warmed pseudo-owner overhead.
class GenericStaticExplicitBench
{
    public static function step<T : int>(T $value): T
    {
        return $value + 1;
    }

    public static function run(int $iterations): int
    {
        $value = 0;
        for ($i = 0; $i < $iterations; $i++) {
            $value = GenericStaticExplicitBench::step::<int>($value);
        }
        return $value;
    }
}

$start = microtime(true);
$value = GenericStaticExplicitBench::run(5000000);
$elapsed = microtime(true) - $start;

echo $value . '|' . $elapsed;
