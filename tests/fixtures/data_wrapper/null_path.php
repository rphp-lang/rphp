<?php
set_error_handler(function ($level, $message) { echo "unexpected:$message\n"; });
foreach (["data:,a\0b", "data:;base64,AA\0A="] as $uri) {
    try { file_get_contents($uri); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
    try { fopen($uri, 'r'); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
    try { file($uri); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
}
