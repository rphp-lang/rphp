<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
class LinkProtocol {
    public function url_stat($path, $flags) { echo "unexpected-stat\n"; return false; }
    public function unlink($path) { echo "unexpected-unlink\n"; return false; }
}
stream_wrapper_register('linkprobe', LinkProtocol::class);
set_error_handler(function ($level, $message) { echo $message, "\n"; });
file_put_contents('source', 'kept');
foreach (['link', 'symlink'] as $name) {
    var_dump($name('linkprobe://target', 'entry'));
    var_dump($name('source', 'linkprobe://entry'));
    var_dump($name('file://source', 'entry'));
}
var_dump(readlink('linkprobe://entry'), readlink('file://source'));
var_dump(file_exists('entry'));
