<?php

$pattern = '~^(?:(?:alpha|beta)-(?:(?:([a-z]+):([0-9]+))|(?:\[(a|b)+\])))(?:/(?:end|stop))$~';
$checksum = 0;
$startedAt = microtime(true);
for ($index = 0; $index < 100_000; $index++) {
    $subject = $index % 2 === 0 ? 'alpha-word:123/end' : 'beta-[abba]/stop';
    $matched = preg_match($pattern, $subject, $groups);
    $checksum += $matched + strlen($groups[0]);
}
$elapsed = microtime(true) - $startedAt;
echo $checksum . '|' . $elapsed;
