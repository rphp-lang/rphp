<?php
$uri = 'data:text/plain,A%0D%0A%0AB%00%0Alast';
foreach ([0, FILE_IGNORE_NEW_LINES, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES] as $flags) {
    foreach (file($uri, $flags) as $line) { echo '[', bin2hex($line), ']'; }
    echo "\n";
}
echo bin2hex(file_get_contents($uri, false, null, -4, 3)), "\n";
$s = fopen($uri, 'r', false, null);
echo bin2hex(fgets($s)), ':', bin2hex(stream_get_contents($s)), "\n";
fclose($s);
