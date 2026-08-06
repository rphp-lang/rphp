<?php
// Structural control: an irregular integer-key stream must materialize the
// general index once and retain exact PHP array behavior.
$n = 1000000;
$values = [];
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$elapsed = microtime(true) - $t;
echo count($values) . '|' . $elapsed;
