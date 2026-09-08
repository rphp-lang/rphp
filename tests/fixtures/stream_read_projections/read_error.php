<?php
$path = tempnam(sys_get_temp_dir(), 'rphp-read-');
$stream = fopen($path, 'wb');
fwrite($stream, 'kept');
set_error_handler(function ($level, $message) { echo $level, ':', $message, "\n"; });
var_dump(fgetc($stream), ftell($stream));
var_dump(fpassthru($stream), ftell($stream));
restore_error_handler();
set_error_handler(function ($level, $message) { throw new Exception('caught:' . $message); });
foreach (['fgetc', 'fpassthru'] as $name) {
    try { $name($stream); echo "unreachable\n"; }
    catch (Throwable $e) { echo $e->getMessage(), "\n"; }
}
restore_error_handler();
var_dump(is_resource($stream), ftell($stream));
fclose($stream);
echo file_get_contents($path), "\n";
unlink($path);
