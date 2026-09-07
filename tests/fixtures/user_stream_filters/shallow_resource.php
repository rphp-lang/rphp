<?php
class ContainerStream {
    public $context;
    private string $label;
    public function stream_open($path, $mode, $options, &$opened): bool {
        $this->label = substr($path, 12);
        return true;
    }
    public function stream_close(): void { echo 'close:', $this->label, "\n"; }
}
stream_wrapper_register('container', ContainerStream::class);
$stream = fopen('container://object', 'r');
$object = (object)['payload' => $stream];
unset($stream);
echo "object-live\n";
unset($object);
echo "object-gone\n";

$items = [(object)['payload' => fopen('container://array', 'r')]];
$copy = $items;
unset($items);
echo "array-copy-live\n";
unset($copy);
echo "array-copy-gone\n";

$stream = fopen('php://memory', 'w+');
$alias = $stream;
unset($stream);
echo 'native:', fwrite($alias, 'ok'), ':', (int)fclose($alias), "\n";
unset($alias);
(object)['payload' => fopen('container://temporary', 'r')];
echo "temporary-gone\n";
['payload' => fopen('container://array-temporary', 'r')];
echo "array-temporary-gone\n";
(object)($shared = ['payload' => fopen('container://shared', 'r')]);
echo "shared-live\n";
unset($shared);
echo "shared-gone\n";
echo "done\n";
