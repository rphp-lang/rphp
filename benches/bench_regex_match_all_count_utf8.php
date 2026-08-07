<?php

$count = 5000;
$subject = "";
for ($index = 0; $index < $count; $index++) {
    $subject .= "uživatel" . $index . " ";
}

$startedAt = microtime(true);
$matched = preg_match_all('/uživatel[0-9]+/', $subject);
$elapsed = microtime(true) - $startedAt;

echo $matched . "|" . $elapsed;
