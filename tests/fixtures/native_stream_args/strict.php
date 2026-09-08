<?php
declare(strict_types=1);
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
$stream = fopen('php://memory', 'w+');
fwrite($stream, 'ABCD');
foreach ([fn() => fread($stream, '2'), fn() => fgets($stream, 2.0), fn() => fwrite($stream, 12), fn() => fwrite($stream, 'xy', '2'), fn() => fseek($stream, '1'), fn() => fseek($stream, 1, '0')] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo $error->getMessage(), "\n"; }
}
var_dump(ftell($stream));
rewind($stream);
var_dump(fgets($stream, null), fwrite($stream, 'Z', null));
fclose($stream);
try { fread($stream, 'bad'); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
