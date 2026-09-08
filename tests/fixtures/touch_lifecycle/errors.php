<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
set_error_handler(function ($level, $message) { echo $level, ':', $message, "\n"; });
file_put_contents('kept', 'same');
foreach (['', 'absent/file', 'kept/child', 'kept/'] as $path) var_dump(touch($path, 123));
try { touch('', []); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
var_dump(touch('', null, 1));
try { touch("bad\0path", [], []); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
try { touch('not-created', [], []); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
var_dump(file_exists('not-created'));
echo file_get_contents('kept'), "\n";
