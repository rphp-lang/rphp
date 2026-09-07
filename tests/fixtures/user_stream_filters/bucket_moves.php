<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class BucketFilter extends php_user_filter {
    function filter($in, $out, &$consumed, $closing): int {
        while ($b = stream_bucket_make_writeable($in)) {
            $consumed += $b->datalen; $copy = clone $b;
            $b->data = 'M'; $copy->data = 'C';
            stream_bucket_append($out, $b); stream_bucket_append($out, $copy); stream_bucket_prepend($out, $b);
            echo get_class($b), ':', $b->datalen, ':', $copy->datalen, "\n";
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('buckets', BucketFilter::class);
$s = fopen('php://memory', 'w+'); stream_filter_append($s, 'buckets', STREAM_FILTER_WRITE);
fwrite($s, 'original'); rewind($s); echo stream_get_contents($s), "\n"; fclose($s);
