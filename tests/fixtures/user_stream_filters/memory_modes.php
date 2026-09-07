<?php
set_error_handler(function () { return true; });
class ReadModeProbe extends php_user_filter {
    function filter($in, $out, &$consumed, $closing): int {
        echo 'read:', (int)$closing, ':';
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('mode.probe', ReadModeProbe::class);
foreach (['php://memory', 'php://temp'] as $uri) {
    foreach (['w', 'rw', 'a', 'r+', '+r', 'za', 'x', 'r', "r\0w"] as $mode) {
        $stream = fopen($uri, $mode);
        echo $uri, ':', json_encode($mode), ':', stream_get_meta_data($stream)['mode'], ':';
        $written = fwrite($stream, 'ok');
        var_dump($written);
        rewind($stream);
        stream_filter_append($stream, 'mode.probe', STREAM_FILTER_READ);
        var_dump(fread($stream, 2));
        fclose($stream);
        echo "closed\n";
    }
}
