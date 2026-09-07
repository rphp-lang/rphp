<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo 'diag:', $message, "\n"; return true; });
class ProjectionProtocol {
    public $filtername;
    public $params;
    function onCreate(): bool { $GLOBALS['probe'] = $this; return true; }
    function filter($in, $out, &$consumed, $closing): int {
        echo 'body:', (int)$closing, "\n";
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
    function onClose(): void { echo "retired\n"; }
}
class FrozenProjection extends ProjectionProtocol {
    public readonly mixed $stream;
    function onCreate(): bool { $this->stream = null; return parent::onCreate(); }
}
class UninitializedProjection extends ProjectionProtocol {
    public readonly mixed $stream;
    function filter($in, $out, &$consumed, $closing): int {
        echo 'initialized:', (int)(new ReflectionProperty($this, 'stream'))->isInitialized($this), "\n";
        return parent::filter($in, $out, $consumed, $closing);
    }
}
class AliasProjection extends ProjectionProtocol {
    public $stream;
    public ?string $held = null;
    function onCreate(): bool { $this->stream =& $this->held; return parent::onCreate(); }
}
class ScopedProjection extends ProjectionProtocol {
    public private(set) mixed $stream = null;
}
class BackedProjection extends ProjectionProtocol {
    public mixed $stream = 'old' {
        get { echo "get\n"; return $this->stream; }
        set { echo 'set:', gettype($value), "\n"; $this->stream = $value; }
    }
}
class VirtualProjection extends ProjectionProtocol {
    public mixed $stream {
        get => null;
        set { echo 'virtual:', gettype($value), "\n"; }
    }
}
class ReadOnlyHookProjection extends ProjectionProtocol {
    public mixed $stream { get => null; }
}
foreach ([FrozenProjection::class, UninitializedProjection::class, AliasProjection::class, ScopedProjection::class, BackedProjection::class, VirtualProjection::class, ReadOnlyHookProjection::class] as $class) {
    echo 'class:', $class, "\n";
    stream_filter_register($class, $class);
    $stream = fopen('php://memory', 'w+');
    stream_filter_append($stream, $class, STREAM_FILTER_WRITE);
    try { var_dump(fwrite($stream, 'x')); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
    try { var_dump(fclose($stream)); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
    unset($stream, $GLOBALS['probe']);
}
