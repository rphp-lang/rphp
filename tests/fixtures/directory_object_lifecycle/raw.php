<?php
chdir(getenv('RPHP_DIRECTORY_SPEC_DIR'));
$folder = "entry-\xff";
mkdir('directory');
file_put_contents('directory/leaf-é', 'x');
symlink('directory', $folder);
$d = dir($folder);
echo bin2hex($d->path), "\n";
$names = [];
while (false !== ($entry = $d->read())) { $names[] = bin2hex($entry); }
sort($names);
echo implode(',', $names), "\n";
$d->rewind();
$names = [];
while (false !== ($entry = readdir($d->handle))) { $names[] = bin2hex($entry); }
sort($names);
echo implode(',', $names), "\n";
$d->close();
