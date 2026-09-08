<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
foreach (['r', 'r+', 'w', 'a', 'nonsense', '', "r\0ignored"] as $mode) {
    $s = fopen('data:text/plain,ABCDE', $mode);
    echo 'mode:', stream_get_meta_data($s)['mode'], "\n";
    echo fread($s, 2), ':', ftell($s), ':', (int) feof($s), "\n";
    var_dump(fwrite($s, 'Z'));
    echo fseek($s, -1, SEEK_END), ':', fread($s, 4), ':', (int) feof($s), "\n";
    echo rewind($s), ':', fread($s, 8), "\n";
    fclose($s);
}
