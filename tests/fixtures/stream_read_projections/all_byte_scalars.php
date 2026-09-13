<?php
$payload = '';
for ($i = 0; $i < 256; ++$i) $payload .= chr($i);
foreach (['php://memory', 'data:application/octet-stream;base64,' . base64_encode($payload)] as $path) {
    $stream = fopen($path, $path === 'php://memory' ? 'w+' : 'r');
    if ($path === 'php://memory') { fwrite($stream, $payload); rewind($stream); }
    $alias = $stream;
    $seen = '';
    $count = 0;
    $valid = true;
    while (($byte = fgetc($alias)) !== false) {
        $valid = $valid && strlen($byte) === 1 && bin2hex($byte) === bin2hex($payload[$count]);
        $seen .= $byte;
        ++$count;
    }
    echo $count, ':', ftell($stream), ':', (int)($valid && $seen === $payload), ':', (int)feof($stream), "\n";
    fclose($stream);
}
