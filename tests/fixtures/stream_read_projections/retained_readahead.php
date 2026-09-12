<?php
$path = tempnam(sys_get_temp_dir(), 'rphp-read-');
$stream = fopen($path, 'r+');
foreach ([10000, 65, 1, 0, 257, 2] as $length) {
    $payload = str_repeat("\x80\x00\xff\n", $length);
    $writer = fopen($path, 'w');
    fwrite($writer, $payload);
    fclose($writer);
    fseek($stream, 0);
    $first = fread($stream, 1);
    $tail = fread($stream, strlen($payload) + 1);
    echo strlen($first . $tail), ':', md5($first . $tail), ':', ftell($stream), ':', (int)feof($stream), "\n";
}
fclose($stream);
unlink($path);
