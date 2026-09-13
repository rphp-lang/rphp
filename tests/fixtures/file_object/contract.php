<?php
// Original differential specimens; every file is local to this invocation.
$files = [];
function specimen_file(string $bytes): string {
    global $files;
    $file = tempnam(sys_get_temp_dir(), 'rphp-file-cursor-');
    $files[] = $file;
    file_put_contents($file, $bytes);
    return $file;
}
function message(string $text): string {
    global $files;
    foreach ($files as $index => $file) $text = str_replace($file, '<file'.$index.'>', $text);
    return $text;
}
function attempt(callable $action): void {
    try { $action(); } catch (Throwable $error) { echo $error::class, ':', message($error->getMessage()), "\n"; }
}
function value_text($value): string {
    return is_string($value) ? 's:'.bin2hex($value) : json_encode($value);
}
function view($file, string $label): void {
    $line = $file->current();
    echo $label, ':', $file->key(), ':', (int)$file->valid(), ':', (int)$file->eof(), ':', value_text($line), "\n";
}
set_error_handler(function($level, $text) { echo 'diag:', $level, ':', message($text), "\n"; return true; });
try {
switch (getenv('RPHP_FILE_OBJECT_CASE')) {
case 'metadata':
    foreach (['__construct','current','next','rewind','valid','key','seek','getFlags','setFlags','getMaxLineLen','setMaxLineLen','getCurrentLine','getChildren','hasChildren'] as $name) {
        $method = new ReflectionMethod(SplFileObject::class, $name);
        echo $name, ':', $method->getNumberOfRequiredParameters(), '/', $method->getNumberOfParameters(), ':';
        foreach ($method->getParameters() as $p) echo $p->getName(), '=', (string)$p->getType(), ',', (int)$p->isOptional(), ';';
        echo ':', $method->hasTentativeReturnType() ? (string)$method->getTentativeReturnType() : (string)$method->getReturnType(), "\n";
    }
    echo SplFileObject::DROP_NEW_LINE, ':', SplFileObject::READ_AHEAD, ':', SplFileObject::SKIP_EMPTY, ':', SplFileObject::READ_CSV, "\n";
    break;
case 'empty-and-termination':
    foreach (['', "A", "A\n", "\n", "A\n\n", "A\r\nB\rC\n"] as $bytes) {
        echo 'input:', bin2hex($bytes), "\n";
        $f = new SplFileObject(specimen_file($bytes));
        echo 'initial:', $f->key(), ':', (int)$f->eof(), "\n";
        for ($i = 0; $i < 5; $i++) { view($f, 'line'); $f->next(); }
    }
    break;
case 'cached-current':
    $f = new SplFileObject(specimen_file("red\ngreen\nblue"));
    view($f, 'first'); view($f, 'again');
    $f->next(); view($f, 'next'); view($f, 'same');
    $f->next(); $f->next(); view($f, 'past');
    $f->rewind(); view($f, 'rewound');
    break;
case 'advance-before-fetch':
    $f = new SplFileObject(specimen_file("zero\none\ntwo\nthree\n"));
    $f->next(); $f->next(); view($f, 'two-next');
    $f->rewind(); $f->next(); view($f, 'rewind-next');
    $f->setFlags(SplFileObject::READ_AHEAD); $f->rewind(); $f->next(); $f->next(); view($f, 'ahead-next');
    break;
case 'flags':
    $path = specimen_file("a\r\n\n\r\nb\n");
    foreach ([0,1,2,4,3,6,7,16] as $flags) {
        $f = new SplFileObject($path); $f->setFlags($flags); echo 'flags:', $f->getFlags(), "\n";
        $f->rewind();
        for ($i = 0; $i < 8 && $f->valid(); $i++) { view($f, 'iter'); $f->next(); }
        view($f, 'end');
    }
    break;
case 'flag-transitions':
    $f = new SplFileObject(specimen_file("x\n\ny\n"));
    view($f, 'raw'); $f->setFlags(1); view($f, 'same-after-flags');
    $f->next(); view($f, 'empty'); $f->setFlags(7); view($f, 'skip-current');
    $f->rewind(); view($f, 'ahead'); $f->setFlags(0); view($f, 'cached-again');
    break;
case 'seek':
    $f = new SplFileObject(specimen_file("a\nb\nc"));
    foreach ([0,2,1,10,0] as $line) { $f->seek($line); view($f, 'seek-'.$line); }
    attempt(function() use ($f) { $f->seek(-1); }); view($f, 'after-invalid');
    $f->setFlags(3); $f->seek(1); view($f, 'flagged');
    break;
case 'seek-direct-read':
    foreach (["a\nb\nc", "a\nb\nc\n"] as $bytes) {
        $f = new SplFileObject(specimen_file($bytes));
        $f->seek(1);
        echo 'direct:', value_text($f->getCurrentLine()), ':', $f->key(), "\n";
        echo 'direct:', value_text($f->getCurrentLine()), ':', $f->key(), "\n";
        $f->seek(3); $f->next();
        echo 'beyond:', $f->key(), ':', (int)$f->valid(), "\n";
        $f->setFlags(2); $f->seek(1);
        echo 'ahead-direct:', value_text($f->getCurrentLine()), ':', $f->key(), "\n";
    }
    break;
case 'line-length':
    $path = specimen_file("abcd\nefgh\ni");
    foreach ([0,1,2,5] as $length) {
        $f = new SplFileObject($path); $f->setMaxLineLen($length); echo 'limit:', $f->getMaxLineLen(), "\n";
        for ($i = 0; $i < 14 && $f->valid(); $i++) { view($f, 'piece'); $f->next(); }
    }
    attempt(function() use ($f) { $f->setMaxLineLen(-1); }); echo 'preserved:', $f->getMaxLineLen(), "\n";
    break;
case 'get-current-line':
    $f = new SplFileObject(specimen_file("a\nb\nc\n"));
    echo 'direct:', value_text($f->getCurrentLine()), ':', $f->key(), "\n";
    echo 'direct:', value_text($f->getCurrentLine()), ':', $f->key(), "\n";
    view($f, 'current'); $f->next(); view($f, 'next');
    $f->rewind(); echo 'rewind-direct:', value_text($f->getCurrentLine()), "\n"; view($f, 'after');
    break;
case 'binary-lines':
    $f = new SplFileObject(specimen_file("\x00\xff\x80\n\xc3\xa9\r\nlast\x00"));
    $f->setMaxLineLen(3);
    for ($i = 0; $i < 10 && $f->valid(); $i++) { view($f, 'binary'); $f->next(); }
    break;
case 'arguments-preserve-state':
    $f = new SplFileObject(specimen_file("a\nb\n")); $f->seek(1); view($f, 'before');
    foreach (['current','next','rewind','valid','key','getFlags','getMaxLineLen','getChildren','hasChildren'] as $method) {
        attempt(function() use ($f, $method) { $f->$method(42); });
    }
    attempt(function() use ($f) { $f->seek([]); });
    attempt(function() use ($f) { $f->setFlags([]); });
    attempt(function() use ($f) { $f->setMaxLineLen([]); }); view($f, 'after');
    $f->seek(line: '0'); view($f, 'named');
    break;
case 'constructor-errors':
    $path = specimen_file('ready');
    foreach ([[''], [$path."\0"], [$path.'.missing'], [$path,'invalid'], [$path,'r',false,[]]] as $args) {
        attempt(function() use ($args) { new SplFileObject(...$args); });
    }
    $f = new SplFileObject(filename: $path, mode: 'r'); view($f, 'named');
    break;
case 'reconstruction':
    $first = specimen_file("one\ntwo"); $second = specimen_file('other');
    $f = new SplFileObject($first); $f->seek(1); view($f, 'before');
    attempt(function() use ($f, $second) { $f->__construct($second); }); view($f, 'after');
    attempt(function() use ($f) { $copy = clone $f; view($copy, 'clone'); });
    attempt(function() use ($f) { serialize($f); });
    break;
case 'string-projection':
    $f = new SplFileObject(specimen_file("a\nb"));
    echo 'cast:', bin2hex((string)$f), ':', bin2hex((string)$f), ':', $f->key(), "\n";
    $f->next(); echo 'next:', bin2hex((string)$f), ':', $f->key(), "\n";
    $f->next(); attempt(function() use ($f) { echo 'end:', bin2hex((string)$f), ':', $f->key(), "\n"; });
    $f->rewind(); $f->setFlags(1); echo 'stripped:', bin2hex((string)$f), "\n";
    break;
case 'direct-eof-error':
    foreach (['', 'a', "a\n"] as $bytes) {
        $f = new SplFileObject(specimen_file($bytes));
        for ($i = 0; $i < 3; $i++) attempt(function() use ($f) {
            echo 'direct:', value_text($f->getCurrentLine()), ':', $f->key(), "\n";
        });
        echo 'state:', $f->key(), ':', (int)$f->eof(), "\n";
    }
    break;
case 'inherited-metadata':
    $path = specimen_file("bytes\n"); $f = new SplFileObject($path);
    echo (int)($f instanceof SplFileInfo), ':', (int)($f instanceof RecursiveIterator), ':', (int)($f instanceof SeekableIterator), "\n";
    echo (int)($f->getPathname() === $path), ':', (int)($f->getBasename() === basename($path)), ':', $f->getSize(), "\n";
    var_dump($f->hasChildren(), $f->getChildren());
    break;
case 'override-line':
    class TaggedLines extends SplFileObject {
        public int $calls = 0;
        public function getCurrentLine(): string { ++$this->calls; return 'tag:'.parent::getCurrentLine(); }
    }
    $f = new TaggedLines(specimen_file("a\nb"));
    view($f, 'first'); view($f, 'cached'); echo 'calls:', $f->calls, "\n";
    $f->next(); view($f, 'next'); echo 'calls:', $f->calls, "\n";
    break;
case 'override-error':
    class FailingLines extends SplFileObject {
        public int $calls = 0;
        public function getCurrentLine(): string { if (++$this->calls === 1) throw new RuntimeException('line-stop'); return parent::getCurrentLine(); }
    }
    $f = new FailingLines(specimen_file("a\nb"));
    attempt(function() use ($f) { view($f, 'failed'); }); view($f, 'retry');
    $f->next(); view($f, 'after'); echo 'calls:', $f->calls, "\n";
    break;
case 'override-invalid-return':
    class InvalidLines extends SplFileObject {
        #[ReturnTypeWillChange]
        public function getCurrentLine(): array { return ['not-a-line']; }
    }
    $f = new InvalidLines(specimen_file("first\nlast"));
    attempt(function() use ($f) { $f->current(); });
    echo 'key:', $f->key(), ':', (int)$f->eof(), "\n";
    attempt(function() use ($f) { $f->current(); });
    break;
case 'lifetime-cycle':
    class OwnedFile extends SplFileObject {
        public $peer;
        public function __destruct() { echo 'destruct:', $this->key(), ':', value_text($this->current()), "\n"; }
    }
    $f = new OwnedFile(specimen_file("a\nb")); $f->peer = $f;
    $weak = WeakReference::create($f); unset($f); gc_collect_cycles();
    echo 'retired:', (int)($weak->get() === null), "\n";
    break;
case 'wrapper-order':
    class CursorWrapper {
        public $context;
        private string $data = "a\nb\n";
        private int $position = 0;
        public function url_stat($path, $flags): array { echo 'url-stat|'; return ['mode' => 0100644, 'size' => 4]; }
        public function stream_open($path, $mode, $options, &$opened): bool { echo 'open|'; return true; }
        public function stream_stat(): array { echo 'stat|'; return []; }
        public function stream_read($count): string { echo 'read|'; $part = substr($this->data, $this->position, $count); $this->position += strlen($part); return $part; }
        public function stream_eof(): bool { echo 'eof|'; return $this->position >= strlen($this->data); }
        public function stream_seek($offset, $whence): bool { echo 'seek|'; if ($whence !== SEEK_SET) return false; $this->position = $offset; return true; }
        public function stream_tell(): int { return $this->position; }
        public function stream_close(): void { echo 'close|'; }
    }
    stream_wrapper_register('cursorprobe', CursorWrapper::class);
    $f = new SplFileObject('cursorprobe://lines'); view($f, 'first'); view($f, 'again');
    $f->next(); view($f, 'next'); $f->rewind(); view($f, 'rewound'); unset($f);
    stream_wrapper_unregister('cursorprobe'); echo "done\n";
    break;
case 'wrapper-stat-missing':
    class MissingStatWrapper {
        public $context;
        public function stream_open($path, $mode, $options, &$opened): bool { echo 'unexpected-open|'; return true; }
    }
    stream_wrapper_register('nostatprobe', MissingStatWrapper::class);
    attempt(function() { new SplFileObject('nostatprobe://lines'); });
    stream_wrapper_unregister('nostatprobe');
    break;
case 'wrapper-cycle':
    class OwnedCursorWrapper {
        public $context;
        public $peer;
        private bool $read = false;
        public function url_stat($path, $flags): array { return ['mode' => 0100644, 'size' => 2]; }
        public function stream_open($path, $mode, $options, &$opened): bool { $GLOBALS['cursor_wrapper'] = $this; return true; }
        public function stream_stat(): array { return []; }
        public function stream_read($count): string { if ($this->read) return ''; $this->read = true; return "a\n"; }
        public function stream_eof(): bool { return $this->read; }
        public function stream_close(): void { echo "close\n"; }
        public function __destruct() { echo "wrapper-destroy\n"; }
    }
    stream_wrapper_register('ownedcursor', OwnedCursorWrapper::class);
    $f = new SplFileObject('ownedcursor://lines'); view($f, 'owned');
    $cursor_wrapper->peer = $f;
    $weakFile = WeakReference::create($f); $weakWrapper = WeakReference::create($cursor_wrapper);
    unset($f, $cursor_wrapper); gc_collect_cycles();
    echo 'retired:', (int)($weakFile->get() === null), ':', (int)($weakWrapper->get() === null), "\n";
    stream_wrapper_unregister('ownedcursor');
    break;
default: throw new RuntimeException('unknown specimen');
}
} catch (Throwable $error) {
    echo $error::class, ':', message($error->getMessage()), "\n";
}
foreach ($files as $file) unlink($file);
