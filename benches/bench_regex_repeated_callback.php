<?php

function redact_regex_match($matches)
{
    return "[redacted]";
}

$count = 5000;
$subject = "";
for ($index = 0; $index < $count; $index++) {
    $subject .= "user" . $index . " ";
}

$startedAt = microtime(true);
$result = preg_replace_callback('/user[0-9]+/', "redact_regex_match", $subject);
$elapsed = microtime(true) - $startedAt;

echo strlen($result) . "|" . $elapsed;
