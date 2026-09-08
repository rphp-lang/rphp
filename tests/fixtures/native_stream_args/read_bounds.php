<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
$stream = fopen('php://memory', 'w+');
fwrite($stream, "A\0B\nnext");
foreach (['fread', 'fgets'] as $name) {
    echo $name, "\n";
    foreach ([0, -7, 1, 3, null, false, true, '2', '2.5', 2.5, '', [], new stdClass, INF] as $length) {
        rewind($stream);
        try {
            $result = $name($stream, $length);
            if (is_string($result)) { echo bin2hex($result), "\n"; }
            else { var_dump($result); }
        } catch (Throwable $error) {
            echo get_class($error), ':', $error->getMessage(), "\n";
        }
        echo 'position:', ftell($stream), "\n";
    }
}
fclose($stream);
foreach (['fread', 'fgets'] as $name) {
    foreach ([0, -7, null, [], new stdClass] as $length) {
        try { $name($stream, $length); }
        catch (Throwable $error) { echo $error->getMessage(), "\n"; }
    }
}
