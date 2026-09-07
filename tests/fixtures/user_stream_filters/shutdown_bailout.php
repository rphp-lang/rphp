<?php
class FailingInput {
    public $context;
    private static $entered = false;
    public function stream_open($path, $mode, $options, &$opened): bool { return true; }
    public function stream_read($length): string { return 'x'; }
    public function stream_set_option($option, $first, $second) {}
    public function stream_stat() {}
    public function stream_eof(): bool {
        if (!self::$entered) {
            self::$entered = true;
            echo "nested read\n";
            include 'failinginput://nested';
        }
        @trigger_error('read stopped', E_USER_ERROR);
    }
    public function stream_close(): void { @trigger_error('close stopped', E_USER_ERROR); }
}
stream_wrapper_register('failinginput', FailingInput::class);
include 'failinginput://outer';
