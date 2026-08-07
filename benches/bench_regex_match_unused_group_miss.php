<?php

$subject = str_repeat("abcž", 64) . "needle";
$iterations = 20000;
$checksum = 0;

$startedAt = microtime(true);
for ($index = 0; $index < $iterations; $index++) {
    $checksum += preg_match('/(missing)/', $subject);
}
$elapsed = microtime(true) - $startedAt;

echo $checksum . "|" . $elapsed;
