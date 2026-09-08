<?php
chdir(getenv('RPHP_DIRECTORY_SPEC_DIR'));
$d = dir('.');
$attempts = [
    function() use ($d) { $d->path = 'elsewhere'; },
    function() use ($d) { unset($d->handle); },
    function() use ($d) { $v =& $d->path; },
    function() use ($d) { $d->path .= '/more'; },
    function() use ($d) { $d->extra = 1; },
    function() use ($d) { return clone $d; },
    function() use ($d) { return serialize($d); },
    function() { return unserialize('O:9:"Directory":0:{}'); },
    function() { return new Directory; },
];
foreach ($attempts as $attempt) {
    try { $attempt(); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
echo $d->path, "\n";
var_dump(is_resource($d->handle));
$d->close();
