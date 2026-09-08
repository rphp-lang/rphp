<?php
chdir(getenv('RPHP_DIRECTORY_SPEC_DIR'));
$d = dir('.');
$alias = $d;
$handle = $d->handle;
unset($d);
echo $alias->path, ':', get_resource_type($handle), "\n";
var_dump($handle === $alias->handle);
closedir($handle);
try { $alias->read(); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
unset($alias);
$next = dir('.');
$nextHandle = $next->handle;
unset($next);
var_dump(is_resource($nextHandle));
closedir($nextHandle);
