<?php
declare(strict_types=1);
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
foreach ([[1, 2], ['integer-string', '12'], ['float', 1.0], ['boolean', 1, true]] as $args) {
    try { touch(...$args); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
var_dump(touch('valid', 123, null), filemtime('valid'), fileatime('valid'));
