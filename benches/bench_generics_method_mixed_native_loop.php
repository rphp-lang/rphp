<?php

// Reified/erased generic method with one raw Long and one borrowed String
// argument. The receiver contract is proven once before the mixed native
// region; the measured loop performs no generic side-table lookup.
class GenericMixedNativeKernel<T>
{
    public function score(int $value, T $key): int
    {
        return $value + strlen($key);
    }
}

function runGenericMixedNative(GenericMixedNativeKernel $kernel, int $limit): int
{
    $totals = ['left' => 0, 'right' => 0];
    $key = 'left';
    $needle = -1;
    for ($i = 0; $i < $limit; $i++) {
        if (($i % 2) == 0) {
            $key = 'right';
        } else {
            $key = 'left';
        }
        $score = $kernel->score($i, $key);
        $totals[$key] = $totals[$key] + $score;
        if ($i === $needle) {
            echo 'never';
        }
    }
    return $totals['left'] + $totals['right'];
}

$kernel = new GenericMixedNativeKernel::<string>();
runGenericMixedNative($kernel, 1000);
$start = microtime(true);
$sum = runGenericMixedNative($kernel, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
