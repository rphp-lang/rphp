<?php
class PendingInput {
    public $context;
    public function stream_open($path, $mode, $options, &$opened): bool { return true; }
    public function stream_close(): void { echo "pending input closed\n"; }
}
stream_wrapper_register('pendinginput', PendingInput::class);
$retained = fopen('pendinginput://document', 'r');
echo "before compilation\n";
include __DIR__ . '/shutdown_invalid.inc';
echo "unreachable\n";
