<?php

$count = 5000;
$subject = "";
for ($index = 0; $index < $count; $index++) {
    $subject .= "user" . $index . " ";
}

$startedAt = microtime(true);
$matched = preg_match_all('/(?P<label>user)([0-9]+)/', $subject, $matches);
$elapsed = microtime(true) - $startedAt;

$checksum = count($matches[0]) + count($matches[1]) + count($matches[2]);
$checksum += count($matches['label']);
echo $matched . ":" . $checksum . "|" . $elapsed;
