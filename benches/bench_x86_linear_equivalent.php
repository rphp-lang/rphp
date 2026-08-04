<?php
// Two-operation shared-IR recurrence with a dynamic bound and invariant CV.
$n = 10000000;
$addend = 7;
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < $n; $i++) {
    $sum += $i + $addend;
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
