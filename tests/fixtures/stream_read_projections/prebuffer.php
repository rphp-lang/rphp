<?php
$path = tempnam(sys_get_temp_dir(), 'rphp-read-');
file_put_contents($path, 'old-data');
$stream = fopen($path, 'rb');
echo fgetc($stream), ':', ftell($stream), "\n";
$writer = fopen($path, 'r+');
fwrite($writer, 'new-data');
fclose($writer);
ob_start();
$count = fpassthru($stream);
$bytes = ob_get_clean();
echo $bytes, ':', $count, ':', ftell($stream), "\n";
fclose($stream);
echo file_get_contents($path), "\n";
unlink($path);
