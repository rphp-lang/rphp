<?php
$path = 'data:,blocked';
$count = 0;
set_error_handler(function ($level, $message) use (&$path, &$count) {
    ++$count;
    $path = 'php://memory';
    throw new Exception($message);
});
try { fopen($path, 'r'); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
echo $count, ':', $path, "\n";
set_error_handler(function ($level, $message) { echo "later:$message\n"; });
var_dump(file_get_contents('data:,later'));
