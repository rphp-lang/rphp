<?php
class TemporaryRangeFilter extends php_user_filter {
    public function filter($in, $out, &$consumed, $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
    public function onClose(): void {
        echo 'close:', $this->params, ':', gettype($this->stream), "\n";
        if ($this->params === 'throw') throw new Exception('temporary-close');
    }
}
stream_filter_register('temporary.range', TemporaryRangeFilter::class);
function temporary_stream($label) {
    $stream = fopen('php://memory', 'w+');
    if ($label !== 'native') {
        stream_filter_append($stream, 'temporary.range', STREAM_FILTER_WRITE, $label);
    }
    return $stream;
}
function padded_stream($left, $stream, $right) { return $stream; }
function finally_stream($stream) {
    try { return padded_stream(2 + 3, $stream, strlen('tail')); }
    finally { echo 'finally:', (int)is_resource($stream), "\n"; }
}

$native = temporary_stream('native');
for ($i = 0; $i < 3; ++$i) {
    rewind(padded_stream($i + 1, $native, strlen('padding')));
    fwrite($native, 'abc');
}
rewind($native);
echo 'native:', fread($native, 3), ':', (int)is_resource($native), "\n";
$alias = finally_stream($native);
unset($native);
echo 'alias:', (int)is_resource($alias), "\n";
fclose($alias);

padded_stream(1 + 2, temporary_stream('ignored'), strlen('padding'));
echo "after-ignored\n";
try { padded_stream(1 + 2, temporary_stream('throw'), strlen('padding')); }
catch (Throwable $error) { echo 'caught:', $error->getMessage(), "\n"; }
echo "after-throw\n";

$filtered = temporary_stream('kept');
$alias = finally_stream($filtered);
unset($filtered);
echo 'filtered-alias:', (int)is_resource($alias), "\n";
unset($alias);
echo "done\n";
