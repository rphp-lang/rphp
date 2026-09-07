<?php
set_error_handler(function ($level, $message) { echo 'warning:', $message, "\n"; return true; });
class CloseFailure extends php_user_filter {
    function filter($input, $output, &$consumed, $closing): int {
        echo 'filter:', $this->params, ':', (int)$closing, "\n";
        while ($bucket = stream_bucket_make_writeable($input)) {
            $consumed += $bucket->datalen;
            stream_bucket_append($output, $bucket);
        }
        if ($closing && $this->params === 'flush') throw new Exception('flush-stop');
        return PSFS_PASS_ON;
    }
    function onClose(): void {
        echo 'retire:', $this->params, "\n";
        if ($this->params === 'retire') throw new Exception('retire-stop');
    }
}
stream_filter_register('close.failure', CloseFailure::class);
foreach (['flush', 'retire'] as $failure) {
    echo 'case:', $failure, "\n";
    $stream = fopen('php://memory', 'w+');
    $first = stream_filter_append($stream, 'close.failure', STREAM_FILTER_WRITE, $failure);
    $second = stream_filter_append($stream, 'close.failure', STREAM_FILTER_WRITE, 'later');
    try { fclose($stream); } catch (Throwable $error) { echo $error->getMessage(), "\n"; }
    echo 'live:', (int)is_resource($stream), ':', (int)is_resource($first), ':', (int)is_resource($second), "\n";
    unset($stream, $first, $second);
    echo "released\n";
}
