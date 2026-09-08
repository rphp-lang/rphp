<?php
$stream = fopen('php://memory', 'w+');
$alias = $stream;
fclose($stream);
foreach (['fgetc', 'fpassthru'] as $name) {
    foreach ([null, false, true, 29, 3.5, 'stream', [], new stdClass, $alias] as $value) {
        try { $name($value); }
        catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
    }
    try { $name(); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
    try { $name($alias, 'extra'); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
    try { $name(unknown: $alias); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
var_dump(is_resource($stream), is_resource($alias));
$directory = opendir(sys_get_temp_dir());
var_dump(fgetc($directory), fpassthru($directory), is_resource($directory));
closedir($directory);
