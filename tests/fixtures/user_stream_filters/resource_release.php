<?php
class OwnedStream {
    public $context;
    private string $label;
    public function stream_open($path, $mode, $options, &$opened): bool {
        $this->label = substr($path, 8);
        echo 'open:', $this->label, "\n";
        return true;
    }
    public function stream_close(): void {
        echo 'close:', $this->label, "\n";
        if ($this->label === 'throws') throw new Exception('close-failed');
    }
}
stream_wrapper_register('owned', OwnedStream::class);
function owned($label) { return fopen('owned://' . $label, 'r'); }

$first = owned('alias');
$second = $first;
unset($first);
echo "alias-live\n";
unset($second);
echo "alias-gone\n";

$first = owned('reference');
$second =& $first;
unset($first);
echo "reference-live\n";
unset($second);
echo "reference-gone\n";

$box = ['nested' => [owned('array')]];
$copy = $box;
unset($box);
echo "copy-live\n";
unset($copy);
echo "copy-gone\n";

$box = (object)['stream' => owned('property')];
unset($box->stream);
echo "property-gone\n";
unset($box);

function retained() {
    $stream = owned('closure');
    return function () use ($stream) { return is_resource($stream); };
}
$callback = retained();
echo 'closure-live:', (int)$callback(), "\n";
unset($callback);
echo "closure-gone\n";

function local_stream($fail) {
    $stream = owned($fail ? 'unwind' : 'return');
    echo "local-live\n";
    if ($fail) throw new Exception('unwind');
}
local_stream(false);
echo "returned\n";
try { local_stream(true); }
catch (Throwable $error) { echo 'caught:', $error->getMessage(), "\n"; }

$stream = owned('replace');
$stream = null;
echo "replaced\n";
$stream = owned('throws');
try { $stream = null; }
catch (Throwable $error) { echo 'caught:', $error->getMessage(), "\n"; }
echo 'after-throw:', gettype($stream), "\n";

$stream = owned('explicit');
$alias = $stream;
fclose($stream);
echo 'explicit:', (int)is_resource($alias), "\n";
unset($stream, $alias);
echo "done\n";
