<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
set_error_handler(function ($level, $message) {
    echo "callback:", $message, "\n";
    touch('nested', 123, 456);
    throw new Exception('interrupted');
});
try { touch('fraction', 1.5); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
var_dump(file_exists('fraction'));
echo filemtime('nested'), ':', fileatime('nested'), "\n";
try { touch('missing/child', 123); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
var_dump(file_exists('missing/child'));
