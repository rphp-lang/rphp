<?php
// App-like object dispatch: multiple objects, conditional logic, mixed access
// Simulates a simplified request processor
class Counter {
    public $n = 0;
    public function bump() { $this->n = $this->n + 1; }
    public function value() { return $this->n; }
}

class Accumulator {
    public $total = 0;
    public function add($v) { $this->total = $this->total + $v; }
    public function result() { return $this->total; }
}

$cnt = new Counter();
$acc = new Accumulator();
$t = microtime(true);
for ($i = 0; $i < 5000000; $i++) {
    $cnt->bump();
    if ($i % 3 == 0) {
        $acc->add($cnt->value());
    }
}
$elapsed = microtime(true) - $t;
echo $acc->result() . '|' . $elapsed;
