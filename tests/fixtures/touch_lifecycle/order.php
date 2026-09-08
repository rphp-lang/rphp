<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
class TimestampPath {
    public function __construct(public $text) {}
    public function __toString(): string { echo "path-conversion\n"; return $this->text; }
}
function timeInput($label, $value) { echo $label, "\n"; return $value; }
var_dump(touch(new TimestampPath('ordered'), timeInput('mtime-eval', 123), timeInput('atime-eval', 456)));
try { touch(new TimestampPath("bad\0"), timeInput('later-evaluated', [])); }
catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { touch(new TimestampPath('not-created'), [], []); }
catch (Throwable $e) { echo $e->getMessage(), "\n"; }
var_dump(file_exists('not-created'));
