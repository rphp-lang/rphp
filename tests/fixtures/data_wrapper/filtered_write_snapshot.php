<?php
class DataWriteSnapshotFilter extends php_user_filter {
    function filter($in, $out, &$consumed, bool $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            $GLOBALS['writePayload'] = str_repeat('changed', 40);
            $consumed += $bucket->datalen;
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('snapshot.write', DataWriteSnapshotFilter::class);
$writePayload = 'original-tail';
$alias =& $writePayload;
$stream = fopen('php://memory', 'w+');
$filter = stream_filter_append($stream, 'snapshot.write', STREAM_FILTER_WRITE);
echo fwrite($stream, $alias, 8), "\n";
stream_filter_remove($filter);
rewind($stream);
var_dump(fread($stream, 100), $alias === str_repeat('changed', 40));
fclose($stream);
