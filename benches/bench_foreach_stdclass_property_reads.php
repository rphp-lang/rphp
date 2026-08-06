<?php
// Holdout for canonical dynamic stdClass receivers in a value-only foreach.
$rows = [];
for ($i = 0; $i < 256; $i++) {
    $rows[] = json_decode('{"value":11,"name":"alpha"}');
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
