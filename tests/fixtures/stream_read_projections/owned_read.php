<?php
$payload = str_repeat("\x00\x7f\x80\xff\n", 13);
// This optimization covers the unbuffered memory backend. Buffered/temporary
// fallbacks are compared with the original slice reader by the stream unit test.
foreach (['php://memory'] as $uri) {
    $observed = [];
    foreach ([0, 1, 63, 64, 65, 9999] as $position) {
        foreach ([1, 2, 63, 64, 65, 66, 8193] as $length) {
            foreach ([false, true] as $prefetch) {
                $stream = fopen($uri, 'w+');
                fwrite($stream, $payload);
                fseek($stream, $position);
                if ($prefetch) fgets($stream, 2);
                $first = fread($stream, $length);
                $afterFirst = [ftell($stream), feof($stream)];
                $second = fread($stream, $length);
                $observed[] = [bin2hex($first), $afterFirst, bin2hex($second), ftell($stream), feof($stream)];
                fclose($stream);
            }
        }
    }
    echo count($observed), ':', md5(json_encode($observed)), "\n";
}
