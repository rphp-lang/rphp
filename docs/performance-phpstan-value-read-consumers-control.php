<?php
class ReadBox {
    public $payload;
    private $inner;
    function __construct($value) { $this->payload = $value; $this->inner = $value; }
    function scoped() { $x = $this->inner; return $x; }
    function direct() { return $this->payload; }
}
$reference = 7;
$a = ['x' => &$reference, 'y' => 5, 'child' => new ReadBox(9)];
$values = [null, false, true, 7, 1.5, 'abc', [1,2], new ReadBox(3)];
$sum = 0;
foreach ($values as $value) {
    $box = new ReadBox($value);
    for ($i=0; $i<100; ++$i) {
        $p = $box->payload;
        $q = $box->scoped();
        $r = $box->direct();
        $sum += (int)($p === $q && $q === $r);
        $x = $a['x'];
        $sum += (int)($x === 7);
        $y = $a['y'];
        $sum += (int)($y === 5);
    }
}
var_dump($sum, $reference, $a['child']->payload);
