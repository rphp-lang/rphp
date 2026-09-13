<?php
$path = tempnam(sys_get_temp_dir(), 'line-projection-');
$payload = str_repeat("a\x00\x80\xff\r\nbc\n", 1000);
foreach (['php://memory', 'php://temp/maxmemory:99999', 'php://temp/maxmemory:3', $path] as $uri) {
    $observed = [];
    foreach ([0, 1, 5, 8190, 8191, 8192, 9000, 9999] as $position) {
        foreach ([2, 3, 4, 32, 64, 65, 66] as $length) {
            $stream = fopen($uri, 'w+');
            fwrite($stream, $payload);
            fseek($stream, $position);
            for ($i = 0; $i < 4; ++$i) {
                $line = fgets($stream, $length);
                $observed[] = [$line === false ? false : bin2hex($line), ftell($stream), feof($stream)];
            }
            $observed[] = [bin2hex(fread($stream, 3)), ftell($stream), feof($stream)];
            fwrite($stream, "Z\n");
            rewind($stream);
            $observed[] = [bin2hex(fgets($stream, 4)), ftell($stream), feof($stream)];
            fclose($stream);
        }
    }
    echo count($observed), ':', md5(json_encode($observed)), "\n";
}
unlink($path);
