<?php
final class SharedTempRows {
    public $rows;

    public function __construct() {
        $this->rows = [];
        for ($i = 0; $i < 4; $i++) {
            $object = new stdClass;
            $object->id = $i + 1;
            $this->rows[] = ['row', $object, ['nested' => $i]];
        }
    }

    public function row($index) {
        return $this->rows[$index & 3];
    }
}

$rows = new SharedTempRows;
$start = microtime(true);
$checksum = 0;
for ($i = 0; $i < 250_000; $i++) {
    $checksum += count($rows->row($i));
    $checksum += $rows->row($i)[1]->id;
}
echo $checksum, '|', microtime(true) - $start, "\n";
