<?php
foreach (["alpha\0tail", base64_decode('AH+A/w=='), str_repeat('word', 20)] as $payload) {
    $alias =& $payload;
    $stream = fopen('php://memory', 'w+');
    $saved = $payload;
    $written = fwrite($stream, $payload, 3);
    rewind($stream);
    echo $written, ':', bin2hex(fread($stream, 100)), ':', (int)($saved === $alias), "\n";
    fclose($stream);
    unset($alias);
}
$payload = 'stable';
set_error_handler(function ($level, $message) use (&$payload) {
    $payload = 'changed';
    echo $level, ':', $message, "\n";
    return true;
});
$stream = fopen('data:,readonly', 'r');
var_dump(fwrite($stream, $payload), $payload);
fclose($stream);
restore_error_handler();
