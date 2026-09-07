<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class RejectFilter extends php_user_filter {
    function onCreate(): bool { echo "reject\n"; return false; }
    function onClose(): void { echo "unexpected-close\n"; }
}
class ThrowFilter extends php_user_filter {
    function onCreate(): bool { throw new Exception('creation'); }
    function onClose(): void { echo "throw-close\n"; }
}
stream_filter_register('reject', RejectFilter::class); stream_filter_register('throw', ThrowFilter::class); stream_filter_register('absent', 'UnregisteredClass');
$s = fopen('php://memory', 'w+');
foreach (['reject','throw','absent','unknown'] as $n) {
    try { var_dump(stream_filter_append($s, $n, STREAM_FILTER_WRITE)); }
    catch (Throwable $e) { echo $e::class, ':', $e->getMessage(), "\n"; }
    echo 'live:', (int)is_resource($s), "\n";
}
fclose($s);
