<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
foreach (['data:text/plain', 'data:plain,x', 'data:;charset=UTF-8,x', 'data:text/plain;tag,x', 'data:text/plain;BASE64,YQ==', 'data:text/plain;base64,YQ==='] as $uri) {
    var_dump(fopen($uri, 'r'));
    var_dump(file_get_contents($uri));
}
$uri = 'data:plain,bad';
set_error_handler(function ($level, $message) use (&$uri) {
    $uri = 'data:,replacement';
    throw new Exception($message);
});
try { fopen($uri, 'r'); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
echo $uri, "\n";
