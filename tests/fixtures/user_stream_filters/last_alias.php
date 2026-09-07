<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) {
    echo 'diag:', $level, ':', $message, "\n";
    return true;
});
class LifetimeFilter extends php_user_filter {
    public function onCreate(): bool {
        echo 'create:', $this->params, ':', gettype($this->stream), "\n";
        $GLOBALS['instance'] = $this;
        return true;
    }
    public function filter($in, $out, &$consumed, $closing): int {
        echo 'call:', $this->params, ':', (int)$closing, ':', gettype($this->stream), "\n";
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
    public function onClose(): void {
        echo 'close:', $this->params, ':', gettype($this->stream), "\n";
    }
}
stream_filter_register('lifetime', LifetimeFilter::class);

$stream = fopen('php://memory', 'w+');
$handle = stream_filter_append($stream, 'lifetime', STREAM_FILTER_WRITE, 'alias');
$other = $stream;
fwrite($stream, 'one');
echo 'after-write:', gettype($GLOBALS['instance']->stream), "\n";
unset($stream);
echo "one-alias-left\n";
unset($other);
echo 'no-aliases:', (int)is_resource($handle), "\n";
unset($handle);
echo "no-filter-handle\n";

$stream = fopen('php://memory', 'w+');
stream_filter_append($stream, 'lifetime', STREAM_FILTER_WRITE, 'container');
$holder = ['resource' => $stream];
unset($stream);
echo "container-held\n";
unset($holder);
echo "container-released\n";

$stream = fopen('php://memory', 'w+');
stream_filter_append($stream, 'lifetime', STREAM_FILTER_WRITE, 'explicit');
$other = $stream;
fclose($stream);
echo 'explicit:', gettype($other), ':', (int)is_resource($other), "\n";
unset($stream, $other);
echo "end\n";
