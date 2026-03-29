<?php
// Property read/write heavy benchmark — measures FetchObjR + AssignObjProp hot path
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
for ($i = 0; $i < 1000000; $i++) {
    $st->record($i);
}
echo $st->sum;
