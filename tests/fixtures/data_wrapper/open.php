<?php
set_error_handler(function ($level, $message) { echo "warning:$message\n"; });
foreach (['data:,a%00b+%2B%fe', 'data://application/octet-stream;base64,AAH/', 'data:text/plain,a%GG%2'] as $uri) {
    $s = fopen($uri, 'rb');
    echo get_resource_type($s), ':', bin2hex(fread($s, 32)), ':', ftell($s), ':', (int) feof($s), "\n";
    rewind($s);
    echo bin2hex(fread($s, 2)), ':', (int) feof($s), "\n";
    fclose($s);
    echo bin2hex(file_get_contents($uri)), "\n";
}
