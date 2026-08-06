<?php
// Holdout for a changing object receiver inside one value-only foreach region.
class BenchmarkForeachPropertyRow {
    public $value;
    public $name;

    public function __construct($value, $name) {
        $this->value = $value;
        $this->name = $name;
    }
}

$rows = [];
for ($i = 0; $i < 256; $i++) {
    $rows[] = new BenchmarkForeachPropertyRow(($i % 8) + 1, 'alpha');
}

$rounds = 20000;
$sum = 0;
$start = microtime(true);
for ($round = 0; $round < $rounds; $round++) {
    foreach ($rows as $row) {
        $sum += $row->value + strlen($row->name);
    }
}
$elapsed = microtime(true) - $start;
echo $sum . '|' . $elapsed;
