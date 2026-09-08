<?php
$context = stream_context_create();
foreach ([fn() => fread($context, []), fn() => fgets($context, 0), fn() => fwrite($context, [], []), fn() => fseek($context, -1), fn() => ftell($context), fn() => fclose($context), fn() => fflush($context)] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo $error->getMessage(), "\n"; }
}
echo get_resource_type($context), "\n";
