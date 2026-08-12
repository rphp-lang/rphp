<?php

class TypedStats
{
    public int $count = 0;
    public int $sum = 0;
    public int $min = 999999999;
    public int $max = 0;

    public function record(int $value): void
    {
        $this->count = $this->count + 1;
        $this->sum = $this->sum + $value;
        if ($value < $this->min) { $this->min = $value; }
        if ($value > $this->max) { $this->max = $value; }
    }
}

$stats = new TypedStats();
$start = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $stats->record($i);
}
$elapsed = microtime(true) - $start;
echo $stats->sum . '|' . $elapsed;
