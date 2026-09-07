<?php
$path = tempnam(sys_get_temp_dir(), 'filter-read-');
$uri = 'php://filter/read=read.empty/resource=' . $path;
set_error_handler(function ($level, $message) use ($uri) {
    echo 'diagnostic:', $level, ':', str_replace([$uri, get_include_path()], ['<filtered>', '<path>'], $message), "\n";
});
class ReadEmptyFilter extends php_user_filter {
    function onClose(): void { echo "closed\n"; }
}
class WritePassFilter extends php_user_filter {
    function filter($in, $out, &$used, $closing): int {
        while ($b = stream_bucket_make_writeable($in)) {
            $used += $b->datalen;
            stream_bucket_append($out, $b);
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('read.empty', ReadEmptyFilter::class);
stream_filter_register('write.pass', WritePassFilter::class);
$base = new php_user_filter;
$empty = fopen('php://memory', 'w+');
$used = null;
var_dump($base->filter($empty, $empty, $used, false));
fclose($empty);
try {
    foreach ([false, true] as $filtered) {
        $s = fopen($path, 'wb');
        if ($filtered) { stream_filter_append($s, 'write.pass'); }
        var_dump(fread($s, 3));
        fwrite($s, '<?php echo "unreachable";');
        fclose($s);
    }
    var_dump(include($uri));
    try { require($uri); }
    catch (Error $e) { echo 'error:', str_replace([$uri, get_include_path()], ['<filtered>', '<path>'], $e->getMessage()), "\n"; }
    set_error_handler(function ($level, $message) { throw new Exception('warning-stop'); });
    try { include($uri); }
    catch (Exception $e) { echo 'exception:', $e->getMessage(), "\n"; }
    echo "alive\n";
} finally { unlink($path); }
