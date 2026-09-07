<?php
$path = tempnam(sys_get_temp_dir(), 'read-cache-');
try {
    file_put_contents($path, 'abcdef');
    $reader = fopen($path, 'rb');
    $writer = fopen($path, 'c+');
    echo fread($reader, 1), ':', filesize($path), "\n";
    ftruncate($writer, 2);
    echo fread($reader, 1), ':', filesize($path), "\n";
    echo fread($reader, 8192), ':', filesize($path), "\n";
    fclose($reader); fclose($writer);
} finally { unlink($path); }
