<?php
// Property R/W: 5M method calls with 4 property accesses each
class Stats {
    public $count = 0;
    public $sum = 0;
    public $min = 999999999;
    public $max = 0;

    public function record($v) {
        $this->count = $this->count + 1;
        $this->sum = $this->sum + $v;
        if ($v < $this->min) { $this->min = $v; }
        if ($v > $this->max) { $this->max = $v; }
    }
}

$st = new Stats();
$t = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $st->record($i);
}
$elapsed = microtime(true) - $t;
echo $st->sum . '|' . $elapsed;
