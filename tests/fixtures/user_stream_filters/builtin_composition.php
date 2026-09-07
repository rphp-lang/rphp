<?php
class MarkerFilter extends php_user_filter {
    function filter($in, $out, &$consumed, $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            $bucket->data = $this->params . $bucket->data;
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
}
foreach (stream_get_filters() as $name) if ($name === 'string.toupper') echo 'builtin:', $name, "\n";
var_dump(stream_filter_register('string.toupper', 'NotLoaded'));
var_dump(stream_filter_register('STRING.TOUPPER', MarkerFilter::class));
var_dump(stream_filter_register('string.*', MarkerFilter::class));
$payload = "a\0z\x80\xff\xc3\xa9";
foreach ([false, true] as $prepend) {
    $stream = fopen('php://memory', 'w+');
    $alias = $stream;
    $upper = stream_filter_append($stream, 'string.toupper', STREAM_FILTER_WRITE);
    if ($prepend) $marker = stream_filter_prepend($alias, 'STRING.TOUPPER', STREAM_FILTER_WRITE, 'm:');
    else $marker = stream_filter_append($alias, 'STRING.TOUPPER', STREAM_FILTER_WRITE, 'm:');
    var_dump(fwrite($alias, $payload));
    rewind($stream);
    echo bin2hex(stream_get_contents($stream)), "\n";
    var_dump(stream_filter_remove($upper), stream_filter_remove($marker));
    fclose($stream);
}
$stream = fopen('php://memory', 'w+');
fwrite($stream, $payload);
rewind($stream);
$filter = stream_filter_append($stream, 'string.toupper', STREAM_FILTER_READ);
echo bin2hex(fread($stream, 2)), ':', ftell($stream), "\n";
echo bin2hex(stream_get_contents($stream)), ':', (int)feof($stream), "\n";
var_dump(stream_filter_remove($filter));
fclose($stream);
$stream = fopen('php://memory', 'w+');
stream_filter_append($stream, 'string.unlisted', STREAM_FILTER_WRITE, 'wild:');
var_dump(fwrite($stream, 'same'));
rewind($stream);
echo stream_get_contents($stream), "\n";
fclose($stream);
