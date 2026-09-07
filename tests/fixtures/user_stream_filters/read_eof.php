<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class TailFilter extends php_user_filter {
    function filter($in, $out, &$consumed, $closing): int {
        echo 'call:', (int)$closing, ':', (int)feof($this->stream), "\n";
        while ($b = stream_bucket_make_writeable($in)) { $consumed += $b->datalen; $b->data = strtoupper($b->data); stream_bucket_append($out, $b); }
        if ($closing) stream_bucket_append($out, stream_bucket_new($this->stream, '!'));
        return PSFS_PASS_ON;
    }
    function onClose(): void { echo "closed\n"; }
}
stream_filter_register('tail', TailFilter::class);
$s = fopen('php://memory', 'w+'); fwrite($s, 'ab'); rewind($s); stream_filter_append($s, 'tail', STREAM_FILTER_READ);
echo 'one:', fread($s, 1), ':', (int)feof($s), "\n";
echo 'rest:', stream_get_contents($s), ':', (int)feof($s), "\n"; fclose($s);
