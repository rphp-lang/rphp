<?php
class PrefetchedUpper extends php_user_filter {
    function filter($in, $out, &$consumed, $closing): int {
        while ($b = stream_bucket_make_writeable($in)) {
            echo 'bucket:', bin2hex($b->data), ':', (int)$closing, "\n";
            $consumed += $b->datalen;
            $b->data = strtoupper($b->data);
            stream_bucket_append($out, $b);
        }
        return PSFS_PASS_ON;
    }
    function onClose(): void { echo "closed\n"; }
}
class PrefetchedReject extends php_user_filter {
    function filter($in, $out, &$consumed, $closing): int {
        while ($b = stream_bucket_make_writeable($in)) { $consumed += $b->datalen; }
        return PSFS_ERR_FATAL;
    }
    function onClose(): void { echo "rejected-close\n"; }
}
stream_filter_register('prefetch.upper', PrefetchedUpper::class);
stream_filter_register('prefetch.reject', PrefetchedReject::class);
set_error_handler(function ($level, $message) { echo 'warning:', $message, "\n"; });
$path = tempnam(sys_get_temp_dir(), 'prefetched-');
try {
    foreach (['php://memory', $path] as $where) {
        foreach (['byte', 'line', 'record'] as $reader) {
            echo ($where === $path ? 'file' : 'memory'), ':', $reader, "\n";
            $s = fopen($where, 'w+');
            fwrite($s, "x\r\nab");
            rewind($s);
            $prefix = match ($reader) {
                'byte' => fread($s, 1),
                'line' => fgets($s),
                'record' => stream_get_line($s, 8192, "\r\n"),
            };
            echo bin2hex($prefix), ':', ftell($s), ':', stream_get_meta_data($s)['unread_bytes'], "\n";
            echo "attach\n";
            stream_filter_append($s, 'prefetch.upper', STREAM_FILTER_READ);
            echo 'after:', ftell($s), ':', bin2hex(stream_get_contents($s)), "\n";
            fclose($s);
        }
    }
    $s = fopen('php://memory', 'w+');
    fwrite($s, "head\nabcd"); rewind($s); fgets($s);
    var_dump(is_resource(stream_filter_append($s, 'prefetch.reject', STREAM_FILTER_READ)));
    echo 'retained:', bin2hex(stream_get_contents($s)), "\n";
    fclose($s);
    file_put_contents($path, 'one OLD');
    $s = fopen($path, 'rb'); fread($s, 4);
    file_put_contents($path, 'two NEW');
    stream_filter_append($s, 'prefetch.upper', STREAM_FILTER_READ);
    echo 'snapshot:', stream_get_contents($s), "\n";
    fclose($s);
} finally { unlink($path); }
