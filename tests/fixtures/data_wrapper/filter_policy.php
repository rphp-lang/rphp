<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
class DataOpenFilter extends php_user_filter {
    function onCreate(): bool { echo "factory\n"; return true; }
    function filter($in, $out, &$consumed, bool $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('data.open', DataOpenFilter::class);
var_dump(file_get_contents('php://filter/read=data.open/resource=data:,payload'));
