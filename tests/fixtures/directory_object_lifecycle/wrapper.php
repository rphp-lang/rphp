<?php
class DirectoryWrapperForSpec {
    public $context;
    private $position = 0;
    public function dir_opendir($path, $options) { echo "open:", $path, "\n"; return true; }
    public function dir_readdir() {
        echo "next\n";
        if ($this->position++ === 0) {
            $nested = dir(getenv('RPHP_DIRECTORY_SPEC_DIR'));
            $nested->close();
            return 'one';
        }
        return false;
    }
    public function dir_rewinddir() { echo "rewind\n"; $this->position = 0; return true; }
    public function dir_closedir() { echo "close\n"; return true; }
}
stream_wrapper_register('dirspec', DirectoryWrapperForSpec::class);
$d = dir('dirspec://folder');
var_dump($d->path, $d->read(), $d->read(), $d->rewind(), $d->read(), $d->close());
stream_wrapper_unregister('dirspec');
