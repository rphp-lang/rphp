<?php
$report = function ($level, $message, $file, $line) {
    echo $message, ':file=', (int)($file === __FILE__), ':line=', (int)($line === $GLOBALS['expectedLine']), "\n";
    return true;
};
set_error_handler($report);
class NestedRejection extends php_user_filter {
    function onCreate(): bool {
        $previous = $GLOBALS['expectedLine'];
        $GLOBALS['expectedLine'] = __LINE__ + 1;
        stream_filter_append($this->params, 'missing.inner', STREAM_FILTER_WRITE);
        $GLOBALS['expectedLine'] = $previous;
        return false;
    }
}
stream_filter_register('nested.reject', NestedRejection::class);
$stream = fopen('php://memory', 'w+');
$GLOBALS['expectedLine'] = __LINE__ + 1;
var_dump(stream_filter_append($stream, 'nested.reject', STREAM_FILTER_WRITE, $stream));
set_error_handler(function ($level, $message, $file, $line) use ($report) {
    $report($level, $message, $file, $line);
    throw new Exception('handler-stop');
});
try {
    $GLOBALS['expectedLine'] = __LINE__ + 1;
    stream_filter_append($stream, 'missing.outer', STREAM_FILTER_READ);
} catch (Throwable $error) { echo $error->getMessage(), "\n"; }
set_error_handler($report);
$GLOBALS['expectedLine'] = __LINE__ + 1;
trigger_error('ordinary-after-filter', E_USER_NOTICE);
fclose($stream);
