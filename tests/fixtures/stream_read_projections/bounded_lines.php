<?php
foreach (['php://memory', 'php://temp/maxmemory:99999', 'php://temp/maxmemory:3'] as $uri) {
    foreach ([2, 64, 65, 66, 8192, 8193, 8194] as $length) {
        $stream = fopen($uri, 'w+');
        fwrite($stream, str_repeat('x', $length - 2) . "\0\x80\xff\nrest\n");
        rewind($stream);
        $first = fgets($stream, $length);
        $position = ftell($stream);
        $eof = feof($stream);
        $second = fgets($stream);
        $third = fread($stream, 99);
        echo json_encode([strlen($first), md5($first), $position, $eof, bin2hex($second), bin2hex($third), ftell($stream), feof($stream)]), "\n";
        rewind($stream);
        echo bin2hex(fread($stream, 1)), ':', ftell($stream), ':', (int)feof($stream), "\n";
        fclose($stream);
    }
}
