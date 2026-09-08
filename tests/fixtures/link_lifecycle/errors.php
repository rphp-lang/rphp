<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
set_error_handler(function ($level, $message) { echo $level, ':', $message, "\n"; });
file_put_contents('source', 'kept');
foreach (['link', 'symlink'] as $name) {
    echo $name, "\n";
    foreach ([['source', 'source'], ['missing', 'missing/entry'], ['', 'entry'], ['source', ''], ['.', 'directory-link']] as $args) {
        try { var_dump($name(...$args)); }
        catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
    }
}
foreach (['missing', 'source', '.', ''] as $path) var_dump(readlink($path));
echo file_get_contents('source'), "\n";
restore_error_handler();
set_error_handler(function ($level, $message) { throw new Exception($message); });
try { link('missing', 'failed'); } catch (Throwable $e) { echo 'caught:', $e->getMessage(), "\n"; }
var_dump(file_exists('failed'));
