<?php
class BenchmarkPropertyRow {
    public $value = 11;
    public $name = 'alpha';
}
$row = new BenchmarkPropertyRow();
$iterations = 5000000;
$sum = 0;
$start = microtime(true);
for ($i = 0; $i < $iterations; $i++) {
    $sum += $row->value + strlen($row->name);
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
