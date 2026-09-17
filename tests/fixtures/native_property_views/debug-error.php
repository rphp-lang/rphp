<?php
set_error_handler(function ($level, $message) {
    echo "diagnostic\n";
    throw new Exception('debug stopped');
});
class EmptyDebugResult {
    function __debugInfo() { echo "first\n"; return null; }
}
class LaterDebugResult {
    function __debugInfo() { echo "second\n"; return []; }
}
try {
    if (getenv('RPHP_DEBUG_ERROR_MODE') === 'dump') {
        var_dump(new EmptyDebugResult);
        var_dump(new LaterDebugResult);
    } else {
        print_r([new EmptyDebugResult, new LaterDebugResult], true);
    }
} catch (Throwable $error) {
    echo 'caught:', $error->getMessage(), "\n";
}
echo "end\n";
