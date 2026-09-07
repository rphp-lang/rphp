<?php
class BufferBoundaryFilter extends php_user_filter {
    private string $held = '';
    function filter($in, $out, &$consumed, $closing): int {
        while ($b = stream_bucket_make_writeable($in)) {
            $consumed += $b->datalen;
            if ($this->params === 'throw') { throw new RuntimeException('buffer callback'); }
            if ($this->params === 'reject') { return PSFS_ERR_FATAL; }
            if ($this->params === 'hold') { $this->held .= $b->data; continue; }
            $b->data = $this->params . '(' . $b->data . ')';
            stream_bucket_append($out, $b);
        }
        if ($this->params === 'hold' && $closing) {
            stream_bucket_append($out, stream_bucket_new($this->stream, strtoupper($this->held)));
        }
        return $this->params === 'hold' && !$closing ? PSFS_FEED_ME : PSFS_PASS_ON;
    }
    function onClose(): void { echo 'close:', $this->params, "\n"; }
}
stream_filter_register('buffer.boundary', BufferBoundaryFilter::class);
function buffered_input() {
    $s = fopen('php://memory', 'w+');
    fwrite($s, "a\nabcd"); rewind($s); fgets($s);
    return $s;
}
foreach (['append', 'prepend'] as $kind) {
    $s = buffered_input();
    stream_filter_append($s, 'buffer.boundary', STREAM_FILTER_READ, 'first');
    $attach = 'stream_filter_' . $kind;
    $attach($s, 'buffer.boundary', STREAM_FILTER_READ, 'second');
    echo $kind, ':', ftell($s), ':', stream_get_meta_data($s)['unread_bytes'], ':', stream_get_contents($s), "\n";
    fclose($s);
}
foreach (['throw', 'reject', 'hold'] as $behavior) {
    $s = buffered_input();
    set_error_handler(function ($level, $message) { throw new RuntimeException('buffer warning'); });
    try { stream_filter_append($s, 'buffer.boundary', STREAM_FILTER_READ, $behavior); }
    catch (RuntimeException $e) { echo $e->getMessage(), "\n"; }
    restore_error_handler();
    echo $behavior, ':', ftell($s), ':', stream_get_contents($s), "\n";
    fclose($s);
}
foreach (['php://memory', 'php://temp'] as $path) {
    $s = fopen($path, 'w+'); fwrite($s, "a\nbcdef"); rewind($s); fgets($s);
    fwrite($s, 'XY');
    echo ftell($s), ':', stream_get_meta_data($s)['unread_bytes'], ':', fread($s, 8), "\n";
    rewind($s); echo bin2hex(stream_get_contents($s)), "\n";
    fclose($s);
}
$path = tempnam(sys_get_temp_dir(), 'cross-buffer-');
try {
    foreach (['php://memory', $path] as $where) {
        $s = fopen($where, 'w+');
        fwrite($s, "a\n" . str_repeat('x', 20000)); rewind($s); fgets($s);
        echo 'cross:', strlen(fread($s, 9000)), ':', ftell($s), ':', stream_get_meta_data($s)['unread_bytes'], "\n";
        echo 'tail:', strlen(fread($s, 20000)), ':', (int)feof($s), "\n";
        fclose($s);
    }
} finally { unlink($path); }
