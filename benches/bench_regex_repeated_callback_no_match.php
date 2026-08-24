<?php

$invocations = 0;
function unreachable_regex_match($matches)
{
    global $invocations;
    $invocations++;
    return $matches[0];
}

$iterations = 200000;
$result = '';
$startedAt = microtime(true);
for ($index = 0; $index < $iterations; $index++) {
    $result = preg_replace_callback('/z/', "unreachable_regex_match", 'abc');
}
$elapsed = microtime(true) - $startedAt;

echo $result . ":" . $invocations . ":" . $iterations . "|" . $elapsed;
