<?php
class ConstantReplayRecords {
    public const RECORD = ['kind' => 7, 'weight' => 11];
    public const KIND = 'kind';
    public const WEIGHT = 'weight';
}
$start = microtime(true);
$sum = 0;
for ($i = 0; $i < 100000; $i++) {
    $record = ConstantReplayRecords::RECORD;
    $sum += $record[ConstantReplayRecords::KIND] + $record[ConstantReplayRecords::WEIGHT];
}
echo $sum, '|', microtime(true) - $start, "\n";
