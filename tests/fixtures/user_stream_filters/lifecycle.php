<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class FrameFilter extends php_user_filter {
    function __construct() { echo "constructor\n"; }
    function onCreate(): bool { echo 'create:', $this->filtername, ':', json_encode($this->params), ':', gettype($this->stream), "\n"; return true; }
    function filter($in, $out, &$consumed, $closing): int {
        echo 'filter:', (int)$closing, ':', gettype($this->stream), ':', $consumed, ':';
        while ($b = stream_bucket_make_writeable($in)) { echo $b->datalen, ','; $consumed += $b->datalen; stream_bucket_append($out, $b); }
        echo "\n"; return PSFS_PASS_ON;
    }
    function onClose(): void { echo 'close:', gettype($this->stream), "\n"; }
}
stream_filter_register('frame', FrameFilter::class);
$s = fopen('php://memory', 'w+'); $f = stream_filter_append($s, 'frame', STREAM_FILTER_WRITE, ['x'=>7]);
echo 'write:', fwrite($s, 'Ab'), "\n"; echo 'flush:', (int)fflush($s), "\n";
rewind($s); echo 'data:', stream_get_contents($s), "\n";
echo 'remove:', (int)stream_filter_remove($f), "\n"; echo 'stream:', (int)is_resource($s), "\n"; fclose($s);
