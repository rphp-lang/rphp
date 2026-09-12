<?php
$results = [];
foreach (['php://memory', 'php://temp/maxmemory:99999', 'php://temp/maxmemory:3'] as $uri) {
    foreach (["A\0BC", "\u{e9} tail", "\xff\0z", str_repeat('r', 80)] as $payload) {
        foreach ([[], [null], [0], [1], [999], [-1]] as $suffix) {
            $stream = fopen($uri, 'w+');
            $alias =& $payload;
            $first = fwrite($stream, $alias, ...$suffix);
            rewind($stream);
            $second = fwrite($stream, $alias, ...$suffix);
            $position = ftell($stream);
            rewind($stream);
            $results[] = [$first, $second, $position, bin2hex(fread($stream, 1000))];
            fclose($stream);
            unset($alias);
        }
    }
}
echo count($results), ':', md5(json_encode($results)), "\n";
$stream = fopen('php://memory', 'w+');
$payload = 'before';
set_error_handler(function ($level, $message) use (&$payload) {
    $payload = 'after';
    echo 'coercion:', $level, "\n";
});
$written = fwrite($stream, $payload, 2.5);
restore_error_handler();
rewind($stream);
echo $written, ':', fread($stream, 100), ':', $payload, "\n";
fclose($stream);
