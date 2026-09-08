<?php
class ProjectionFailure extends php_user_filter {
    public function filter($in, $out, &$consumed, $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            throw new Exception('filter-stopped');
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('projection.failure', ProjectionFailure::class);
foreach (['fgetc', 'fpassthru'] as $name) {
    $stream = fopen('php://memory', 'w+');
    fwrite($stream, 'input'); rewind($stream);
    stream_filter_append($stream, 'projection.failure', STREAM_FILTER_READ);
    echo $name, "\n";
    try { $name($stream); echo "unreachable\n"; }
    catch (Throwable $e) { echo $e->getMessage(), "\n"; }
    var_dump(is_resource($stream));
    fclose($stream);
}
