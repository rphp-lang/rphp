<?php

function retain_regex_match($matches)
{
    static $retainedRegexMatch = null;
    $previous = $retainedRegexMatch;
    $retainedRegexMatch = $matches;
    return $previous === null ? "[redacted]" : $previous[0];
}

$count = 5000;
$subject = "";
for ($index = 0; $index < $count; $index++) {
    $subject .= "user" . $index . " ";
}

$startedAt = microtime(true);
$result = preg_replace_callback('/user[0-9]+/', "retain_regex_match", $subject);
$elapsed = microtime(true) - $startedAt;

echo strlen($result) . "|" . $elapsed;
