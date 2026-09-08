<?php
$stream = fopen('php://memory', 'w+');
fwrite($stream, 'source'); rewind($stream);
$saved = $stream;
set_error_handler(function ($level, $message) use (&$stream) {
    echo $level, ':', $message, "\n";
    $stream = fopen('php://memory', 'w+');
    fwrite($stream, 'replacement'); rewind($stream);
});
var_dump(fread($stream, 2.5));
restore_error_handler();
var_dump(ftell($saved), ftell($stream));
fclose($saved); fclose($stream);
$stream = fopen('php://memory', 'w+');
set_error_handler(function ($level, $message) { throw new Exception('stop:' . $message); });
try { fwrite($stream, null); }
catch (Exception $error) { echo $error->getMessage(), "\n"; }
restore_error_handler();
var_dump(ftell($stream), is_resource($stream));
fclose($stream);
