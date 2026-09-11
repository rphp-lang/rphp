<?php
function same($actual, $expected) {
    if ($actual !== $expected) throw new Exception('mismatch: ' . json_encode([$actual, $expected]));
}
function rejected($operation, $class, $message) {
    try { $operation(); } catch (Throwable $error) {
        same(get_class($error), $class); same($error->getMessage(), $message); return;
    }
    throw new Exception('missing ' . $class);
}
function names($iterator) {
    $result = [];
    for ($iterator->rewind(); $iterator->valid(); $iterator->next()) $result[] = $iterator->getFilename();
    return $result;
}
function position($iterator, $name) {
    $all = names($iterator);
    $iterator->seek(array_search($name, $all, true));
}
$case = getenv('RPHP_DIRECTORY_CASE');
if ($case === 'cursor') {
    $it = new DirectoryIterator('tree///');
    $all = names($it); $sorted = $all; sort($sorted);
    same($sorted, ['.', '..', 'alpha.txt', 'beta.bin']);
    same($it->key(), 4); same($it->valid(), false); same($it->current() === $it, true);
    same($it->getPath(), 'tree//'); same($it->getPathname(), '');
    $it->next(); same($it->key(), 5);
    position($it, 'alpha.txt');
    $alias = $it->current(); same($alias === $it, true);
    same($it->getPathname(), 'tree///alpha.txt');
    same([$it->getBasename('.txt'), $it->getExtension(), $it->getSize()], ['alpha', 'txt', 5]);
    $it->next(); same($alias->getFilename(), $it->getFilename());
    same((string)$it, $it->getFilename());
} elseif ($case === 'seek') {
    $it = new DirectoryIterator('tree'); $all = names($it);
    foreach ([2, 0, -1, 1, 0] as $index) {
        $it->seek($index); same($it->key(), max(0, $index)); same($it->getFilename(), $all[max(0, $index)]);
    }
    $it->seek(4); same($it->valid(), false); same($it->key(), 4);
    rejected(fn() => $it->seek(50), 'OutOfBoundsException', 'Seek position 50 is out of range');
    same($it->key(), 4); $it->seek(0); same($it->valid(), true);
    $diagnostics = [];
    set_error_handler(function ($level, $message) use (&$diagnostics) { $diagnostics[] = [$level, $message]; return true; });
    $it->seek(1.5); same($it->key(), 1); $it->seek('2'); same($it->key(), 2); $it->seek(null); same($it->key(), 0);
    restore_error_handler();
    same($diagnostics, [[8192, 'Implicit conversion from float 1.5 to int loses precision'], [8192, 'DirectoryIterator::seek(): Passing null to parameter #1 ($offset) of type int is deprecated']]);
} elseif ($case === 'flags') {
    $it = new FilesystemIterator('tree'); same($it->getFlags(), 4096);
    $all = names($it); $sorted = $all; sort($sorted); same($sorted, ['alpha.txt', 'beta.bin']);
    $it->rewind(); $a = $it->current(); $b = $it->current();
    same(get_class($a), 'SplFileInfo'); same($a === $b, false); same($a->getPathname(), $it->key());
    foreach ([0x10, 0x80, 0xf0] as $mode) { $it->setFlags($mode); same($it->current() === $it, true); }
    $it->setFlags(0x120); same($it->current(), 'tree/' . $it->getFilename()); same($it->key(), $it->getFilename());
    $it->setFlags(-1); same($it->getFlags(), 0x7ff0);
    $it->setFlags(0x10000); same($it->getFlags(), 0);
    position($it, '.'); $it->setFlags(4096); same($it->getFilename(), '.');
    $it->next(); same($it->isDot(), false);
    $it->rewind(); same($it->isDot(), false); $it->seek(2); same($it->valid(), false);
    rejected(fn() => $it->seek(3), 'OutOfBoundsException', 'Seek position 3 is out of range');
} elseif ($case === 'clone') {
    foreach ([new DirectoryIterator('tree///'), new FilesystemIterator('tree///')] as $it) {
        position($it, 'alpha.txt'); $clone = clone $it;
        same($clone === $it, false); same($clone->getFilename(), 'alpha.txt');
        same($clone->getPath(), 'tree/'); same($clone->getPathname(), 'tree//alpha.txt');
        $it->next(); same($clone->getFilename(), 'alpha.txt');
        $clone->rewind(); same($it->getFilename() === 'alpha.txt', false);
    }
    class CloneDirectory extends DirectoryIterator {
        public array $payload = [1];
        function __clone() { $this->payload[] = 2; $this->rewind(); }
    }
    $it = new CloneDirectory('tree'); position($it, 'beta.bin'); $clone = clone $it;
    same($it->payload, [1]); same($clone->payload, [1, 2]); same($it->getFilename(), 'beta.bin');
    same(method_exists(DirectoryIterator::class, '__clone'), false);
} elseif ($case === 'factory') {
    class ChosenInfo extends SplFileInfo {
        public static int $calls = 0;
        function __construct($path) { self::$calls++; parent::__construct($path); }
    }
    $it = new FilesystemIterator('tree'); $it->setInfoClass(ChosenInfo::class);
    position($it, 'alpha.txt'); $a = $it->current(); $b = $it->current();
    same(ChosenInfo::$calls, 2); same($a === $b, false); same($a->getPathname(), 'tree/alpha.txt');
    $copy = clone $it; $c = $copy->current(); same(ChosenInfo::$calls, 3); same(get_class($c), 'ChosenInfo');
    $it->next(); same($a->getFilename(), 'alpha.txt'); same($copy->getFilename(), 'alpha.txt');
} elseif ($case === 'invalid') {
    foreach (['DirectoryIterator', 'FilesystemIterator'] as $class) {
        rejected(fn() => new $class(''), 'ValueError', "$class::__construct(): Argument #1 (\$directory) must not be empty");
        rejected(fn() => new $class("tree\0x"), 'ValueError', "$class::__construct(): Argument #1 (\$directory) must not contain any null bytes");
        rejected(fn() => new $class('absent'), 'UnexpectedValueException', "$class::__construct(absent): Failed to open directory: No such file or directory");
        rejected(fn() => new $class('tree/alpha.txt'), 'UnexpectedValueException', "$class::__construct(tree/alpha.txt): Failed to open directory: Not a directory");
    }
    $it = new DirectoryIterator('tree');
    rejected(fn() => $it->__construct('absent'), 'Error', 'Directory object is already initialized');
    rejected(fn() => $it->__construct([]), 'TypeError', 'DirectoryIterator::__construct(): Argument #1 ($directory) must be of type string, array given');
    same($it->valid(), true);
} elseif ($case === 'uninitialized') {
    class BareDirectory extends DirectoryIterator { function __construct() {} }
    $it = new BareDirectory;
    foreach (['valid', 'key', 'current', 'next', 'rewind', 'isDot', 'getFilename', 'getSize', '__toString'] as $method)
        rejected(fn() => $it->$method(), 'Error', 'Object not initialized');
    same([$it->getPath(), $it->getPathname(), $it->getRealPath()], ['', '', false]);
    rejected(fn() => $it->seek([]), 'TypeError', 'DirectoryIterator::seek(): Argument #1 ($offset) must be of type int, array given');
    rejected(fn() => $it->getBasename([]), 'TypeError', 'DirectoryIterator::getBasename(): Argument #1 ($suffix) must be of type string, array given');
    same($it->__debugInfo(), ["\0SplFileInfo\0pathName" => '']);
    class BareFilesystem extends FilesystemIterator { function __construct() {} }
    $bare = new BareFilesystem;
    same($bare->getFlags(), 0);
    $bare->setFlags(17); same($bare->getFlags(), 16);
    same([$bare->getPath(), $bare->getPathname(), $bare->getRealPath()], ['', '', false]);
    same($bare->__debugInfo(), ["\0SplFileInfo\0pathName" => '']);
    $copy = clone $bare; same($copy->getFlags(), 16);
    same($bare->current() === $bare, true);
    foreach (['key', 'getSize'] as $method)
        rejected(fn() => $bare->$method(), 'Error', 'Object not initialized');
    $bare->setFlags(0x100); same($bare->key(), '');
    rejected(fn() => $bare->current(), 'Error', 'Object not initialized');
    $bare->setFlags(0x20);
    rejected(fn() => $bare->current(), 'Error', 'Object not initialized');
    (new ReflectionMethod(FilesystemIterator::class, '__construct'))->invoke($bare, 'tree');
    same([$bare->getFlags(), $bare->valid()], [4096, true]);
    same($copy->getFlags(), 16);
    foreach ([[new BareDirectory, 'DirectoryIterator'], [new BareFilesystem, 'FilesystemIterator']] as [$failed, $owner]) {
        $constructor = new ReflectionMethod($owner, '__construct');
        rejected(fn() => $constructor->invoke($failed, 'absent///'), 'UnexpectedValueException', "$owner::__construct(absent///): Failed to open directory: No such file or directory");
        same([$failed->getPath(), $failed->getPathname(), $failed->getRealPath()], ['absent//', '', false]);
        same($failed->__debugInfo(), ["\0SplFileInfo\0pathName" => '', "\0DirectoryIterator\0glob" => false, "\0RecursiveDirectoryIterator\0subPathName" => '']);
        rejected(fn() => $failed->valid(), 'Error', 'Object not initialized');
        rejected(fn() => $failed->getFilename(), 'Error', 'Object not initialized');
        rejected(fn() => $failed->getSize(), 'RuntimeException', 'SplFileInfo::getSize(): stat failed for absent///');
        rejected(fn() => $failed->getFileInfo(), 'RuntimeException', 'Could not open file');
        same($failed->getPathInfo(), null);
        same($failed->__debugInfo(), ["\0SplFileInfo\0pathName" => '', "\0SplFileInfo\0fileName" => '', "\0DirectoryIterator\0glob" => false, "\0RecursiveDirectoryIterator\0subPathName" => '']);
        rejected(fn() => $constructor->invoke($failed, 'tree'), 'Error', 'Directory object is already initialized');
        if ($failed instanceof FilesystemIterator) {
            same($failed->getFlags(), 4096); same($failed->key(), 'absent///'); same($failed->rewind(), null);
            rejected(fn() => $failed->current(), 'RuntimeException', 'Could not open file');
            $failed->setFlags(0x20); same($failed->current(), 'absent///');
        } else rejected(fn() => $failed->rewind(), 'Error', 'Object not initialized');
        $warnings = [];
        set_error_handler(function ($level, $message) use (&$warnings) { $warnings[] = $message; return true; });
        try { $copy = clone $failed; throw new Exception('missing clone error'); }
        catch (UnexpectedValueException $error) { same($error->getMessage(), 'Failed to open directory "absent//"'); }
        restore_error_handler(); same($warnings, ['main(absent//): Failed to open directory: No such file or directory']);
    }
    $bare = new BareFilesystem;
    same($bare->rewind(), null); $bare->setFlags(0x10);
    (new ReflectionMethod(SplFileInfo::class, '__construct'))->invoke($bare, 'plain/name');
    same([$bare->getPath(), $bare->getPathname()], ['plain', 'plain/name']);
    rejected(fn() => (new ReflectionMethod(FilesystemIterator::class, '__construct'))->invoke($bare, 'tree'), 'Error', 'Directory object is already initialized');
} elseif ($case === 'hooks') {
    class HookDirectory extends DirectoryIterator {
        public static array $events = [];
        function rewind(): void { self::$events[] = 'rewind'; parent::rewind(); }
        function valid(): bool { self::$events[] = 'valid'; return parent::valid(); }
        function current(): mixed { self::$events[] = 'current'; return parent::current(); }
        function key(): mixed { self::$events[] = 'key'; return parent::key(); }
        function next(): void { self::$events[] = 'next'; parent::next(); }
    }
    $it = new HookDirectory('tree'); $count = 0;
    foreach ($it as $key => $value) { same($value === $it, true); if (++$count === 2) break; }
    same(HookDirectory::$events, ['rewind', 'valid', 'current', 'key', 'next', 'valid', 'current', 'key']);
    rejected(function () use ($it) { foreach ($it as &$value) {} }, 'Error', 'An iterator cannot be used with foreach by reference');
    same(count(HookDirectory::$events), 8);
} elseif ($case === 'exhausted') {
    foreach ([new DirectoryIterator('tree'), new FilesystemIterator('tree')] as $it) {
        names($it);
        same($it->__debugInfo(), ["\0SplFileInfo\0pathName" => '', "\0DirectoryIterator\0glob" => false, "\0RecursiveDirectoryIterator\0subPathName" => '']);
        same([$it->getPath(), $it->getPathname(), $it->getFilename(), $it->getExtension(), $it->getBasename()], ['tree', '', '', '', '']);
        same([$it->getType(), $it->isFile(), $it->isDir(), $it->isDot()], ['dir', false, true, false]);
        same($it->getSize(), filesize('tree')); same($it->getRealPath(), realpath('tree'));
        same($it->__debugInfo(), ["\0SplFileInfo\0pathName" => '', "\0SplFileInfo\0fileName" => '', "\0DirectoryIterator\0glob" => false, "\0RecursiveDirectoryIterator\0subPathName" => '']);
        rejected(fn() => $it->getFileInfo(), 'RuntimeException', 'Could not open file'); same($it->getPathInfo(), null);
        if ($it instanceof FilesystemIterator) {
            same($it->key(), 'tree/'); rejected(fn() => $it->current(), 'RuntimeException', 'Could not open file');
            $it->setFlags(0x20); same($it->current(), 'tree/');
            $it->setFlags(0x10); same($it->current() === $it, true);
        }
    }
} elseif ($case === 'parent-path') {
    $it = new DirectoryIterator('tree'); $name = $it->getFilename();
    (new ReflectionMethod(SplFileInfo::class, '__construct'))->invoke($it, 'changed/name');
    same([$it->getFilename(), $it->getPath(), $it->getPathname()], [$name, 'changed', 'changed/name']);
    $it->next(); same($it->getPathname(), 'changed/' . $it->getFilename());
    same($it->getFileInfo()->getPathname(), $it->getPathname()); same($it->getPathInfo()->getPathname(), 'changed');
    (new ReflectionMethod(SplFileInfo::class, '__construct'))->invoke($it, 'plain');
    $warnings = [];
    set_error_handler(function ($level, $message) use (&$warnings) { $warnings[] = $message; return true; });
    rejected(fn() => clone $it, 'UnexpectedValueException', 'Failed to open directory ""');
    restore_error_handler(); same($warnings, []);
    same([$it->getPath(), $it->getPathname(), $it->valid()], ['', 'plain', true]);
} elseif ($case === 'raw') {
    foreach (new DirectoryIterator('raw') as $entry) if (!$entry->isDot())
        same([bin2hex($entry->getFilename()), bin2hex($entry->getPathname()), bin2hex($entry->getExtension()), $entry->getSize()], ['6e616d652dff2e80', '7261772f6e616d652dff2e80', '80', 8]);
    foreach (new FilesystemIterator('raw', 0x1120) as $key => $entry)
        same([bin2hex($key), bin2hex($entry)], ['6e616d652dff2e80', '7261772f6e616d652dff2e80']);
} elseif ($case === 'rename') {
    $it = new DirectoryIterator('tree'); $before = names($it); rename('tree', 'moved');
    same(names($it), $before);
    $warnings = [];
    set_error_handler(function ($level, $message) use (&$warnings) { $warnings[] = [$level, str_contains($message, 'Failed to open directory: No such file or directory')]; return true; });
    rejected(fn() => clone $it, 'UnexpectedValueException', 'Failed to open directory "tree"');
    restore_error_handler(); same($warnings, [[2, true]]); same(names($it), $before);
} elseif ($case === 'removed') {
    $it = new DirectoryIterator('empty'); rmdir('empty'); $it->rewind();
    same([$it->valid(), $it->key(), $it->getFilename()], [false, 0, '']);
    rejected(fn() => $it->seek(2), 'OutOfBoundsException', 'Seek position 2 is out of range');
    set_error_handler(function () { throw new LogicException('open callback'); });
    rejected(fn() => clone $it, 'LogicException', 'open callback');
    restore_error_handler(); same($it->valid(), false);
} elseif ($case === 'uri-root') {
    $base = 'file://' . getcwd() . '/tree';
    $it = new DirectoryIterator($base); position($it, 'alpha.txt');
    same($it->getPath(), $base); same($it->getPathname(), $base . '/alpha.txt');
    same($it->getSize(), 5); same($it->getRealPath(), false);
    $copy = clone $it; same($copy->getPath(), $base); same($copy->getFilename(), 'alpha.txt');
    rejected(fn() => new DirectoryIterator('file://.'), 'UnexpectedValueException', 'DirectoryIterator::__construct(): Remote host file access not supported, file://.');
    $root = new DirectoryIterator('/'); same($root->getPath(), '/'); same($root->getPathname(), '//' . $root->getFilename());
} elseif ($case === 'metadata') {
    $it = new DirectoryIterator('tree'); position($it, 'alpha.txt');
    same($it->getInode(), fileinode('tree/alpha.txt'));
    same($it->getPerms(), fileperms('tree/alpha.txt'));
    same($it->isFile(), true); same($it->isDir(), false);
    mkdir('links'); symlink('../tree/alpha.txt', 'links/shortcut');
    $link = new DirectoryIterator('links'); position($link, 'shortcut');
    same($link->isLink(), true); same($link->getLinkTarget(), '../tree/alpha.txt');
    same($link->getSize(), 5); same($link->getRealPath(), realpath('tree/alpha.txt'));
} elseif ($case === 'reflection') {
    foreach ([DirectoryIterator::class, FilesystemIterator::class] as $class) {
        same(is_a($class, SplFileInfo::class, true), true); same(is_a($class, SeekableIterator::class, true), true);
        same((new ReflectionMethod($class, 'valid'))->hasTentativeReturnType(), true);
    }
    $ctor = new ReflectionMethod(FilesystemIterator::class, '__construct');
    same([$ctor->getNumberOfRequiredParameters(), $ctor->getNumberOfParameters()], [1, 2]);
    same($ctor->getParameters()[1]->getDefaultValue(), 4096);
    same(FilesystemIterator::CURRENT_MODE_MASK, 240); same(FilesystemIterator::KEY_MODE_MASK, 3840);
    same(FilesystemIterator::OTHER_MODE_MASK, 28672); same(FilesystemIterator::FOLLOW_SYMLINKS, 16384);
} else { throw new Exception('unknown case'); }
echo $case, ":ok\n";
