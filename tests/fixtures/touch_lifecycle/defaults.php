<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
$before = time();
foreach ([[], [null], [null, null], [123], [123, null], [123, 456]] as $index => $args) {
    $path = 'item' . $index;
    var_dump(touch($path, ...$args));
    $mtime = filemtime($path); $atime = fileatime($path);
    if ($index < 3) var_dump($mtime >= $before && $mtime <= time(), $atime === $mtime);
    else echo $mtime, ':', $atime, "\n";
}
try { touch('must-not-exist', null, 12); }
catch (Throwable $e) { echo $e->getMessage(), "\n"; }
var_dump(file_exists('must-not-exist'));
