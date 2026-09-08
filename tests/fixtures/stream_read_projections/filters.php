<?php
$path = tempnam(sys_get_temp_dir(), 'rphp-read-');
file_put_contents($path, "one\x00two");
$stream = fopen($path, 'rb');
echo fgetc($stream), "\n";
$filter = stream_filter_append($stream, 'string.toupper', STREAM_FILTER_READ);
echo bin2hex(fgetc($stream)), "\n";
ob_start();
$count = fpassthru($stream);
$bytes = ob_get_clean();
echo $count, ':', bin2hex($bytes), "\n";
var_dump(fgetc($stream), fpassthru($stream));
stream_filter_remove($filter);
fclose($stream);
unlink($path);
class ProjectionFilter extends php_user_filter {
    public function filter($in, $out, &$consumed, $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            $bucket->data = strtoupper($bucket->data);
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('projection.upper', ProjectionFilter::class);
$stream = fopen('data:,wxyz', 'r');
stream_filter_append($stream, 'projection.upper', STREAM_FILTER_READ);
echo fgetc($stream), ':', ftell($stream), "\n";
ob_start();
$count = fpassthru($stream);
$bytes = ob_get_clean();
echo $count, ':', $bytes, ':', ftell($stream), "\n";
fclose($stream);
