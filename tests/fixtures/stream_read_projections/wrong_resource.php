<?php
$context = stream_context_create();
foreach (['fgetc', 'fpassthru'] as $name) {
    try { $name($context); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
echo get_resource_type($context), "\n";
