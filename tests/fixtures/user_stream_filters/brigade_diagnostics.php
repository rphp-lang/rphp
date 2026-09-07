<?php
set_error_handler(function ($level, $message) { echo 'warning:', $message, "\n"; return true; });
class BrigadeResult extends php_user_filter {
    function filter($input, $output, &$used, $closing): int {
        if ($this->params[0]) {
            while ($bucket = stream_bucket_make_writeable($input)) {
                $used += $bucket->datalen;
                if ($this->params[0] === 2) stream_bucket_append($output, $bucket);
            }
        }
        if ($this->params[1] === 7 && !$closing) throw new Exception('callback-stop');
        return $this->params[1] === 7 ? PSFS_PASS_ON : $this->params[1];
    }
}
stream_filter_register('brigade.result', BrigadeResult::class);
foreach ([PSFS_ERR_FATAL, PSFS_FEED_ME, PSFS_PASS_ON, 7] as $status) {
    foreach ([0, 1, 2] as $action) {
        echo 'case:', $status, ':', $action, "\n";
        $stream = fopen('php://memory', 'w+');
        stream_filter_append($stream, 'brigade.result', STREAM_FILTER_WRITE, [$action, $status]);
        try { var_dump(fwrite($stream, 'probe')); }
        catch (Throwable $error) { echo $error->getMessage(), "\n"; }
        fclose($stream);
    }
}
class RejectedStreamSlot {
    public $filtername;
    public $params;
    public int $stream = 8;
    function filter($input, $output, &$used, $closing): int { echo "not-called\n"; return PSFS_PASS_ON; }
}
stream_filter_register('rejected.slot', RejectedStreamSlot::class);
$stream = fopen('php://memory', 'w+');
stream_filter_append($stream, 'rejected.slot', STREAM_FILTER_WRITE);
try { fwrite($stream, 'blocked'); }
catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
try { fclose($stream); }
catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
restore_error_handler();
foreach (['rejected.slot', 'brigade.result'] as $name) {
    $stream = fopen('php://memory', 'w+');
    stream_filter_append($stream, $name, STREAM_FILTER_WRITE, [0, 7]);
    error_clear_last();
    try { @fwrite($stream, 'diagnosed'); }
    catch (Throwable $error) { echo 'pending:', $error::class, "\n"; }
    echo 'recorded:', error_get_last()['message'], "\n";
    try { fclose($stream); } catch (Throwable $error) {}
}
set_error_handler(function ($level, $message) { echo 'throws:', $message, "\n"; throw new Exception('warning-stop'); });
$stream = fopen('php://memory', 'w+');
stream_filter_append($stream, 'brigade.result', STREAM_FILTER_WRITE, [0, PSFS_PASS_ON]);
try { fwrite($stream, 'leftover'); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
fclose($stream);
echo "finished\n";
