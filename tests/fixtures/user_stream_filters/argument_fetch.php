<?php
set_error_handler(function ($severity, $message) {
    if ($severity === E_WARNING) {
        echo 'warning:', $message, "\n";
        $GLOBALS['later'] = 'ready';
    }
    return true;
});

echo 'missing:', (int)is_resource($missing), "\n";
echo 'snapshot:', str_replace('a', 'b', $later, $count), ':', $count, "\n";
unset($later);
echo 'second:', str_replace('a', 'b', $later, $count), ':', $count, "\n";
echo 'handler-write:', $later, "\n";

$items = [2, 1];
sort($items);
echo 'reference:', implode(',', $items), "\n";

$stream = fopen('php://memory', 'w+');
$alias =& $stream;
echo 'positional:', (int)is_resource($alias), "\n";
echo 'named:', (int)is_resource(value: $alias), "\n";
$predicate = 'is_resource';
echo 'dynamic:', (int)$predicate($alias), "\n";
fclose($stream);
echo 'closed:', (int)is_resource($alias), "\n";
restore_error_handler();
