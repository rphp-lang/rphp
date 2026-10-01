<?php
$previous = new Error('previous');
for ($i = 0; $i < 3; $i++) {
    $exception = new Exception('outer', 2, $previous);
    $error = new ErrorException('outer', 2, E_WARNING, 'source.php', 9, $previous);
    echo (int)($exception->getPrevious() === $previous), (int)($error->getPrevious() === $previous), ':', $error->getFile(), ':', $error->getLine(), '|';
}
foreach ([fn() => new Exception('x', 0, new stdClass), fn() => new ErrorException('x', 0, E_WARNING, [], 2)] as $factory) {
    try { $factory(); } catch (TypeError $e) { echo 'type|'; }
}
echo (int)((new Exception)->getPrevious() === null), (int)((new ErrorException)->getPrevious() === null);
