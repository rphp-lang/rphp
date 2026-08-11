<?php

// Direct Long generic method whose substituted contract is specialized once
// at typed-region entry and whose body is then composed into the native loop.
class GenericNativeKernel<T>
{
    public function transform(T $value, int $scale): T
    {
        return ($value * $scale) + 7;
    }
}

$kernel = new GenericNativeKernel::<int>();
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < 10000000; $i++) {
    $sum += $kernel->transform($i, 73);
}
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
