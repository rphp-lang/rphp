<?php
function snapshotLine($stream, $length) {
    $line = fgets($stream, $length);
    echo json_encode([$line === false ? false : bin2hex($line), ftell($stream),
        feof($stream), stream_get_meta_data($stream)['unread_bytes']]), "\n";
}
foreach (["", "a", "a\n", "ab\r\n\0\xfftail"] as $text) {
    $stream = fopen('php://memory', 'w+');
    fwrite($stream, $text); rewind($stream);
    snapshotLine($stream, 3);
    snapshotLine($stream, 4);
    snapshotLine($stream, 65);
    snapshotLine($stream, 2);
    fclose($stream);
}
$stream = fopen('php://memory', 'w+'); $alias = $stream;
fwrite($stream, "ab\ncd\n"); rewind($stream);
snapshotLine($stream, 4);
ftruncate($alias, 1);
snapshotLine($stream, 4);
snapshotLine($alias, 4);
rewind($stream); snapshotLine($alias, 4);
fclose($stream);
$stream = fopen('php://memory', 'w+');
fwrite($stream, str_repeat('x', 8191) . "\nlast"); rewind($stream);
snapshotLine($stream, 2);
fseek($stream, 8191);
snapshotLine($stream, 2);
snapshotLine($stream, 5);
snapshotLine($stream, 5);
fclose($stream);
$stream = fopen('data://text/plain;base64,YQpiCg==', 'r');
foreach ([3, 3, 3] as $length) {
    $line = fgets($stream, $length);
    echo json_encode([$line === false ? false : bin2hex($line), ftell($stream),
        stream_get_meta_data($stream)['unread_bytes']]), "\n";
}
fclose($stream);
