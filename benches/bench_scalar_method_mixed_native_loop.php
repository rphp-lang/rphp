<?php

// Ordinary-signature control for the generic mixed native method workload.
class ScalarMixedNativeKernel
{
    public function score(int $value, string $key): int
    {
        return $value + strlen($key);
    }
}

function runScalarMixedNative(ScalarMixedNativeKernel $kernel, int $limit): int
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

$kernel = new ScalarMixedNativeKernel();
runScalarMixedNative($kernel, 1000);
$start = microtime(true);
$sum = runScalarMixedNative($kernel, 10000000);
$elapsed = microtime(true) - $start;

echo $sum . '|' . $elapsed;
