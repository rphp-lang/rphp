<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
set_error_handler(function ($level, $message) { echo $level, ':', $message, "\n"; });
foreach ([[1, '123', true], ['fraction', 1.5, 2.5], ['bad-mtime', [], 4], ['bad-atime', 4, []], ['bad-numeric', 'x'], [null, 3], [[], 3]] as $args) {
    try { var_dump(touch(...$args)); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
echo filemtime('1'), ':', fileatime('1'), "\n";
echo filemtime('fraction'), ':', fileatime('fraction'), "\n";
