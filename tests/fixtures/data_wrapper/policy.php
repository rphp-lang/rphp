<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
var_dump(ini_get('allow_url_fopen'), ini_set('allow_url_fopen', '1'), ini_get('allow_url_fopen'));
var_dump(fopen('data:,hello', 'r') !== false);
var_dump(file_get_contents('data:,hello'));
var_dump(file('data:,line%0A'));
$s = fopen('php://memory', 'w+');
echo fwrite($s, 'local'), ':', rewind($s), ':', fread($s, 8), "\n";
fclose($s);
