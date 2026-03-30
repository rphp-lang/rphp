<?php
// Method call chain: 3 methods per iteration, 5M iterations
// Measures InitMethodCall + DoFcall overhead for scalar-return methods
class Chain {
    public $val = 0;
    public function inc($n) {
        $this->val = $this->val + $n;
        return $this->val;
    }
    public function dec($n) {
        $this->val = $this->val - $n;
        return $this->val;
    }
    public function get() {
        return $this->val;
    }
}

$c = new Chain();
$t = microtime(true);
$sum = 0;
for ($i = 0; $i < 5000000; $i++) {
    $c->inc(3);
    $c->dec(1);
    $sum += $c->get();
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
