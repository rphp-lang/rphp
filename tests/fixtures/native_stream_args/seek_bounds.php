<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
$stream = fopen('php://memory', 'w+');
fwrite($stream, 'abcdef');
foreach ([[1, SEEK_SET], [-1, SEEK_SET], [-2, SEEK_END], [1, SEEK_CUR], [0, 99], ['2', 0], [2.5, 0], [null, 0], [[], 0], [0, []], [0, null]] as $args) {
    rewind($stream);
    try { var_dump(fseek($stream, $args[0], $args[1])); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
    echo 'position:', ftell($stream), "\n";
}
fclose($stream);
foreach ([[-1, 0], [[], 0], [0, []], [0, 99]] as $args) {
    try { fseek($stream, $args[0], $args[1]); }
    catch (Throwable $error) { echo $error->getMessage(), "\n"; }
}
