<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
set_error_handler(function ($level, $message) { echo $level, ':', $message, "\n"; });
file_put_contents('1', 'numeric');
var_dump(link(1, 2), symlink(1, 3), readlink(3));
foreach (['link', 'symlink'] as $name) {
    foreach ([[null, []], ["bad\0", []], ['1', null], [[], 'new']] as $args) {
        try { var_dump($name(...$args)); }
        catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
    }
}
foreach ([null, [], new stdClass, "bad\0"] as $value) {
    try { var_dump(readlink($value)); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
