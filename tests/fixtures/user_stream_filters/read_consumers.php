<?php
class ConsumerMap extends php_user_filter {
    function filter($in, $out, &$used, $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            $used += $bucket->datalen;
            $bucket->data = strtoupper($bucket->data);
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('consumer.map', ConsumerMap::class);
function input_stream(string $text) {
    $stream = fopen('php://memory', 'w+');
    fwrite($stream, $text);
    rewind($stream);
    stream_filter_append($stream, 'consumer.map', STREAM_FILTER_READ);
    return $stream;
}
$stream = input_stream("ab\ncd<>ef\ngh");
echo 'line:', bin2hex(fgets($stream, 3)), ':', ftell($stream), "\n";
echo 'line:', bin2hex(fgets($stream)), ':', ftell($stream), "\n";
echo 'record:', stream_get_line($stream, 20, '<>'), ':', ftell($stream), "\n";
echo 'line:', bin2hex(fgets($stream)), ':', ftell($stream), "\n";
echo 'rest:', stream_get_contents($stream), ':', (int)feof($stream), "\n";
fclose($stream);

$stream = input_stream("ab,\"cd\nef\",gh\n\nlast,row\n");
while (($row = fgetcsv($stream, null, ',', '"', '')) !== false) {
    echo 'csv:', json_encode($row), ':', ftell($stream), "\n";
}
fclose($stream);

$stream = input_stream('n=' . str_repeat('a', 8200) . '<><>tail');
echo 'long:', strlen(stream_get_line($stream, 9000, '<>')), ':', ftell($stream), "\n";
echo 'empty:', json_encode(stream_get_line($stream, 9000, '<>')), ':', ftell($stream), "\n";
$tail = stream_get_line($stream, 0);
echo 'tail:', strlen($tail), ':', substr($tail, -6), ':', (int)feof($stream), "\n";
fclose($stream);

$source = input_stream('abcdef');
$destination = fopen('php://memory', 'w+');
stream_filter_append($destination, 'consumer.map', STREAM_FILTER_WRITE);
echo 'copy:', stream_copy_to_stream($source, $destination, 3, 1), ':', ftell($source), ':', ftell($destination), "\n";
echo 'copy:', stream_copy_to_stream($source, $destination), ':', (int)feof($source), "\n";
rewind($destination);
echo 'result:', stream_get_contents($destination), "\n";
fclose($source);
fclose($destination);

$stream = input_stream("17 4.5 word\n");
echo 'scan:', json_encode(fscanf($stream, '%d %f %s')), "\n";
fclose($stream);
$stream = fopen('php://memory', 'w+');
stream_filter_append($stream, 'consumer.map', STREAM_FILTER_WRITE);
echo 'print:', fprintf($stream, '%s:%d', 'item', 3), ':', vfprintf($stream, '-%s', ['end']), "\n";
rewind($stream);
echo 'printed:', stream_get_contents($stream), "\n";
fclose($stream);
