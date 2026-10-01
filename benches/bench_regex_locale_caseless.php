<?php

$pattern = '~^(?:(?:alpha|beta)-(?:(?:([a-z]+):([0-9]+))|(?:\[(a|b)+\])))(?:/(?:end|stop))$~i';
$checksum = 0;
$startedAt = microtime(true);
for ($index = 0; $index < 100_000; $index++) {
    $subject = $index % 2 === 0 ? 'ALPHA-Word:123/END' : 'BETA-[AbBa]/STOP';
    $matched = preg_match($pattern, $subject, $groups);
    $checksum += $matched + strlen($groups[0]);
}
$elapsed = microtime(true) - $startedAt;
echo $checksum . '|' . $elapsed;
