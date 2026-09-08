<?php
function inspectCall($name, $stream) {
    try {
        switch ($name) {
            case 'fread': $result = fread($stream, 2); break;
            case 'fgets': $result = fgets($stream); break;
            case 'fwrite': $result = fwrite($stream, 'ok'); break;
            case 'fseek': $result = fseek($stream, 0); break;
            default: $result = $name($stream);
        }
        var_dump($result);
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
$closed = fopen('php://memory', 'w+');
$alias = $closed;
fclose($closed);
foreach (['fclose', 'feof', 'ftell', 'rewind', 'fseek', 'fread', 'fgets', 'fwrite', 'fflush'] as $name) {
    echo $name, "\n";
    foreach ([null, false, true, 23, 2.5, 'stream', [], new stdClass, $alias] as $value) {
        inspectCall($name, $value);
    }
}
var_dump(is_resource($closed), is_resource($alias));
$live = fopen('php://memory', 'w+');
var_dump(fwrite($live, 'still live'), ftell($live));
fclose($live);
