<?php
$stream = fopen('php://memory', 'w+');
fwrite($stream, 'content'); rewind($stream);
ob_start(function ($bytes) { return '[' . $bytes . ']'; });
echo 'before:';
$result = fpassthru($stream);
echo ':after:', $result;
ob_end_flush();
echo "\n";
var_dump(ftell($stream));
fclose($stream);
