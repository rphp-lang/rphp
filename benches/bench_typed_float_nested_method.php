<?php

// Holdout for a typed Double method calling another typed Double method on
// the same receiver. The inner dispatch must retain its canonical class and
// method-cache guards when the two bodies are flattened into one native loop.
class FloatPipeline
{
    public function scaleAndShift(float $value, float $scale): float
    {
        return ($value * $scale) + 1.0;
    }

    public function calculate(float $value, float $scale): float
    {
        return ($this->scaleAndShift($value, $scale) * 0.5) + 2.0;
    }
}

$pipeline = new FloatPipeline();
$scale = 2.0;
$total = 0.0;
$start = microtime(true);

for ($i = 0; $i < 5000000; ++$i) {
    $total += $pipeline->calculate($i * 0.5, $scale);
}
$elapsed = microtime(true) - $start;

echo $total . '|' . $elapsed;
