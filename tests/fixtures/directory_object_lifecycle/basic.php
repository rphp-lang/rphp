<?php
chdir(getenv('RPHP_DIRECTORY_SPEC_DIR'));
file_put_contents('pear', 'p');
file_put_contents('apple', 'a');
$d = dir('.');
echo get_class($d), ':', $d->path, ':', get_resource_type($d->handle), "\n";
$entries = [];
while (false !== ($name = $d->read())) { $entries[] = $name; }
sort($entries);
echo implode(',', $entries), "\n";
var_dump($d->read(), $d->rewind());
$again = [];
while (false !== ($name = readdir($d->handle))) { $again[] = $name; }
sort($again);
var_dump($entries === $again, $d->close(), is_resource($d->handle));
foreach (['read', 'rewind', 'close'] as $method) {
    try { $d->$method(); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
unlink('pear'); unlink('apple');
