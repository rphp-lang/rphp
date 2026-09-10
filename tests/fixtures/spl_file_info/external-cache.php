<?php
$info = new SplFileInfo('item.bin');
$copy = clone $info;
echo $info->getSize(), "\n";
flush();
for ($attempt = 0; $attempt < 2000 && !file_exists('advance'); ++$attempt) usleep(1000);
if (!file_exists('advance')) exit(1);
echo $info->getSize(), ':', $copy->getSize(), ':', filesize('item.bin'), "\n";
clearstatcache();
echo $info->getSize(), ':', $copy->getSize(), ':', filesize('item.bin'), "\n";
