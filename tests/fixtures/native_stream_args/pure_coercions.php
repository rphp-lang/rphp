<?php
set_error_handler(function ($level, $message) { echo $level, ':', $message, "\n"; });
$stream = fopen('php://memory', 'w+');
fwrite($stream, "abcd\n");
foreach (['2', " \t+2\r\n", 2.0, true, false, '2e0', '2.0', "\u{00a0}2", '2x'] as $length) {
    rewind($stream);
    try { echo bin2hex(fread($stream, $length)), "\n"; }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
    echo 'position:', ftell($stream), "\n";
}
fclose($stream);
foreach (['2', " \t+2\r\n", 2.0, true, false, '2e0', '2.5'] as $length) {
    foreach (['fread', 'fgets', 'fseek', 'fwrite'] as $function) {
        try {
            if ($function === 'fwrite') { $function($stream, 'safe', $length); }
            else { $function($stream, $length); }
        } catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
    }
}
restore_error_handler();
