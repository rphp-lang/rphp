<?php
chdir(getenv('RPHP_DIRECTORY_SPEC_DIR'));
set_error_handler(function($code, $message) { echo $code, ':', $message, "\n"; return true; });
foreach ([
    function() { return dir(); },
    function() { return dir([]); },
    function() { return dir("x\0y"); },
    function() { return dir('', 42); },
    function() { return dir(''); },
    function() { return dir('absent'); },
    function() { return dir(null); },
] as $attempt) {
    try { var_dump($attempt()); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
$d = dir(directory: '.', context: null);
foreach (['read', 'rewind', 'close'] as $method) {
    try { $d->$method(1); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
$d->close();
