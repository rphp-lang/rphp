<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
class StreamText { function __toString(): string { return 'text'; } }
foreach ([0, -3, 2, null, false, true, '2', '2.5', 2.5, [], 'bad'] as $length) {
    $stream = fopen('php://memory', 'w+');
    try { var_dump(fwrite($stream, "A\0B\xff", $length)); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
    echo 'position:', ftell($stream), "\n";
    rewind($stream);
    echo bin2hex(fread($stream, 20)), "\n";
    fclose($stream);
}
foreach ([null, false, true, 25, 2.5, new StreamText, [], new stdClass] as $data) {
    $stream = fopen('php://memory', 'w+');
    try { var_dump(fwrite($stream, $data)); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
    echo 'position:', ftell($stream), "\n";
    fclose($stream);
}
foreach ([[[], []], ['ok', -1], [null, 1]] as $args) {
    try { fwrite($stream, $args[0], $args[1]); }
    catch (Throwable $error) { echo $error->getMessage(), "\n"; }
}
