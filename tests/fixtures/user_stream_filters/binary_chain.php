<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class MarkerFilter extends php_user_filter {
    function filter($in, $out, &$consumed, $closing): int {
        while ($b = stream_bucket_make_writeable($in)) { $consumed += $b->datalen; $b->data = $this->params . $b->data; stream_bucket_append($out, $b); }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('marker.*', MarkerFilter::class);
$s = fopen('php://memory', 'w+');
stream_filter_append($s, 'marker.a', STREAM_FILTER_WRITE, 'A');
stream_filter_prepend($s, 'marker.b', STREAM_FILTER_WRITE, 'B');
echo fwrite($s, "\x00\x80\xff"), "\n";
rewind($s); echo bin2hex(stream_get_contents($s)), "\n"; fclose($s);
