<?php
function ap_trace_loader($name) { throw new Exception('unavailable:' . $name); }
spl_autoload_register('ap_trace_loader');
for ($mode = 0; $mode < 3; ++$mode) {
    echo "mode:$mode\n";
    try {
        if ($mode === 0) class_exists('ApTraceMissing');
        if ($mode === 1) spl_autoload_call('ApTraceMissing');
        if ($mode === 2) { $name = 'ApTraceMissing'; new $name; }
    } catch (Throwable $error) {
        echo $error->getMessage(), "\n";
        foreach ($error->getTrace() as $frame) {
            echo isset($frame['file']) ? 'user:' : 'internal:', $frame['function'], ':', $frame['args'][0], "\n";
        }
    }
}
