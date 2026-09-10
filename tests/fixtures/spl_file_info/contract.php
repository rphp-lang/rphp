<?php
set_error_handler(function ($level, $message) { echo "diagnostic:", $level, ":", $message, "\n"; return true; });
function attempt($callback) {
    try { var_dump($callback()); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}
$case = getenv('RPHP_FILE_INFO_CASE');
if ($case === 'lexical') {
    foreach (['', '/', '///', '.', './', '..', 'one//two///', './one/item.tar.gz', '.hidden', 'item.', 'one\\two.ext'] as $path) {
        $info = new SplFileInfo($path);
        echo json_encode([$path, $info->getPathname(), $info->getPath(), $info->getFilename(), $info->getExtension(), $info->getBasename(), $info->getBasename('.gz'), $info->getPathInfo()?->getPathname()]), "\n";
    }
} elseif ($case === 'arguments') {
    foreach ([null, 17, true, [], "bad\0tail"] as $path) attempt(function () use ($path) { return (new SplFileInfo($path))->getPathname(); });
    $info = new SplFileInfo('folder/item.bin');
    attempt(fn() => $info->getFilename('surplus'));
    attempt(fn() => $info->getBasename([]));
    attempt(fn() => $info->__construct("bad\0tail"));
    echo $info, "\n";
    attempt(fn() => $info->__construct('next/name'));
    echo $info, "\n";
} elseif ($case === 'uninitialized') {
    class EmptyInfo extends SplFileInfo { public function __construct() {} }
    $info = new EmptyInfo;
    foreach (['getFilename', 'getPath', 'getExtension', 'getBasename', 'getPathname', 'getSize', 'getLinkTarget', 'getRealPath', 'isFile', 'getFileInfo', 'getPathInfo', '__toString', '_bad_state_ex'] as $method) attempt(fn() => $info->$method());
    foreach ($info->__debugInfo() as $key => $value) echo bin2hex($key), ':', $value, "\n";
    attempt(fn() => $info->setInfoClass());
    attempt(fn() => $info->setFileClass());
} elseif ($case === 'missing') {
    $info = new SplFileInfo('absent-fixture.info');
    foreach (['getPerms', 'getInode', 'getSize', 'getOwner', 'getGroup', 'getATime', 'getMTime', 'getCTime', 'getType', 'getLinkTarget', 'getRealPath', 'isFile', 'isDir', 'isLink', 'isReadable', 'isWritable', 'isExecutable'] as $method) { echo $method, ':'; attempt(fn() => $info->$method()); }
} elseif ($case === 'metadata') {
    $info = new SplFileInfo('item.bin');
    $stat = stat('item.bin');
    foreach (['getPerms'=>'mode', 'getInode'=>'ino', 'getSize'=>'size', 'getOwner'=>'uid', 'getGroup'=>'gid', 'getATime'=>'atime', 'getMTime'=>'mtime', 'getCTime'=>'ctime'] as $method => $field) echo $method, ':', (int) ($info->$method() === $stat[$field]), "\n";
    foreach (['getType', 'isFile', 'isDir', 'isLink', 'isReadable', 'isWritable', 'isExecutable'] as $method) { echo $method, ':'; var_dump($info->$method()); }
    echo (int) ($info->getRealPath() === realpath('item.bin')), "\n";
    $directory = new SplFileInfo('folder');
    echo $directory->getType(), ':', (int) $directory->isDir(), "\n";
} elseif ($case === 'symlink') {
    $info = new SplFileInfo('alias.bin');
    var_dump($info->getLinkTarget(), $info->getType(), $info->isLink(), $info->isFile(), $info->getSize());
    echo (int) ($info->getInode() === fileinode('item.bin')), ':', (int) ($info->getRealPath() === realpath('item.bin')), "\n";
    $broken = new SplFileInfo('broken.bin');
    var_dump($broken->getType(), $broken->isLink(), $broken->isFile(), $broken->getRealPath(), $broken->getLinkTarget());
    attempt(fn() => (new SplFileInfo('item.bin'))->getLinkTarget());
} elseif ($case === 'cache') {
    $info = new SplFileInfo('item.bin');
    $copy = clone $info;
    var_dump($info->getSize());
    $stream = fopen('item.bin', 'r+');
    ftruncate($stream, 2);
    var_dump($info->getSize(), $copy->getSize());
    clearstatcache();
    var_dump($info->getSize(), $copy->getSize());
    fclose($stream);
    unlink('item.bin');
    attempt(fn() => $info->getSize());
    echo $info->getFilename(), "\n";
} elseif ($case === 'factories') {
    class ChosenInfo extends SplFileInfo {
        public function __construct(string $path) { echo 'construct:', $path, "\n"; parent::__construct($path); }
    }
    $info = new SplFileInfo('folder/item.bin');
    foreach ([$info->getFileInfo(), $info->getPathInfo()] as $other) echo get_class($other), ':', $other, "\n";
    $info->setInfoClass(ChosenInfo::class);
    foreach ([$info->getFileInfo(), $info->getPathInfo()] as $other) echo get_class($other), ':', $other, "\n";
    attempt(fn() => $info->getFileInfo(SplFileInfo::class));
    $info->setInfoClass();
    echo get_class($info->getFileInfo()), "\n";
} elseif ($case === 'factory-errors') {
    $info = new SplFileInfo('item.bin');
    foreach (['AbsentInfoType', stdClass::class, '', null, 1] as $class) {
        attempt(fn() => $info->setInfoClass($class));
        attempt(fn() => get_class($info->getFileInfo($class)));
        attempt(fn() => $info->setFileClass($class));
    }
    attempt(fn() => $info->setFileClass());
    echo get_class($info->getFileInfo()), "\n";
} elseif ($case === 'clone') {
    class CloneInfo extends SplFileInfo { public $label = 'old'; public function __clone() { $this->label = 'new'; } }
    $info = new CloneInfo('folder/item.bin');
    $info->setInfoClass(CloneInfo::class);
    $copy = clone $info;
    $info->__construct('other');
    echo $info, ':', $info->label, ':', $copy, ':', $copy->label, ':', get_class($copy->getFileInfo()), "\n";
    echo get_class($info->getFileInfo()), "\n";
    $copy->setInfoClass(SplFileInfo::class);
    $copy->__construct('copy-only');
    echo $info, ':', get_class($info->getFileInfo()), ':', $copy, ':', get_class($copy->getFileInfo()), "\n";
} elseif ($case === 'native-policy') {
    class PolicyInfo extends SplFileInfo {}
    foreach ([new SplFileInfo('item.bin'), new PolicyInfo('item.bin')] as $info) attempt(fn() => serialize($info));
    attempt(fn() => unserialize('O:11:"SplFileInfo":0:{}'));
    $incomplete = unserialize('O:11:"SplFileInfo":0:{}', ['allowed_classes' => false]);
    echo serialize($incomplete), "\n";
    attempt(fn() => (new SplFileInfo('item.bin'))->_bad_state_ex());
    attempt(fn() => (new ReflectionClass(SplFileInfo::class))->newInstanceWithoutConstructor()->getPathname());
} elseif ($case === 'debug') {
    class DebugInfo extends SplFileInfo { public $extra = 'member'; }
    $info = new DebugInfo('folder/item.bin/');
    foreach ($info->__debugInfo() as $key => $value) echo bin2hex($key), ':', $value, "\n";
    echo (string) $info, ':', (int) (bool) $info, "\n";
    var_dump(array_keys((array) $info));
} elseif ($case === 'raw-bytes') {
    $path = "raw-\xff.\x80";
    $info = new SplFileInfo($path);
    foreach ([$info->getPathname(), $info->getFilename(), $info->getExtension(), $info->getBasename(".\x80"), (string) $info] as $value) echo bin2hex($value), "\n";
    var_dump($info->getSize(), $info->isFile());
    $link = new SplFileInfo('raw-link');
    echo bin2hex($link->getLinkTarget()), ':', (int) ($link->getRealPath() === $info->getRealPath()), "\n";
} elseif ($case === 'strict-order') {
    eval('declare(strict_types=1); attempt(fn() => new SplFileInfo(42)); attempt(fn() => (new SplFileInfo("item.bin"))->getBasename(1));');
    class UntouchedInfo extends SplFileInfo { function __construct() {} }
    $info = new UntouchedInfo;
    attempt(fn() => $info->getBasename([]));
    attempt(fn() => $info->getFileInfo(stdClass::class));
    attempt(fn() => $info->getPathInfo(stdClass::class));
    attempt(fn() => $info->setInfoClass([]));
    attempt(fn() => $info->setInfoClass(new stdClass));
} elseif ($case === 'callback-state') {
    $info = new SplFileInfo('before');
    class ReenterInfoPath {
        function __construct(public $target) {}
        function __toString(): string { $this->target->__construct('nested'); return 'outer'; }
    }
    $info->__construct(new ReenterInfoPath($info));
    echo $info, "\n";
    class ThrowingInfo extends SplFileInfo {
        public function __construct($path) { echo 'factory:', $path, "\n"; throw new RuntimeException('factory stopped'); }
    }
    $info->setInfoClass(ThrowingInfo::class);
    attempt(fn() => $info->getFileInfo());
    echo $info, "\n";
    class NoParentInfo extends SplFileInfo { function __construct($path) { echo 'without-parent:', $path, "\n"; } }
    $info->setInfoClass(NoParentInfo::class);
    $copy = $info->getFileInfo();
    attempt(fn() => $copy->getFilename());
    echo $copy->getPathname(), "\n";
} elseif ($case === 'factory-native-construction') {
    class PrivateInfo extends SplFileInfo { private function __construct($path) { echo 'private:', $path, "\n"; } }
    class InheritedPrivateInfo extends PrivateInfo {}
    abstract class AbstractFactoryInfo extends SplFileInfo {}
    $info = new SplFileInfo('item.bin');
    foreach ([PrivateInfo::class, InheritedPrivateInfo::class, AbstractFactoryInfo::class] as $class) {
        $info->setInfoClass($class);
        $copy = $info->getFileInfo();
        echo get_class($copy), ':', $copy->getPathname(), "\n";
    }
} elseif ($case === 'raw-errors') {
    foreach (["missing-\xff", "missing-\xc3\xa9"] as $path) {
        $info = new SplFileInfo($path);
        foreach (['getSize', 'getType', 'getLinkTarget'] as $method) {
            try { $info->$method(); } catch (Throwable $error) { echo get_class($error), ':', bin2hex($error->getMessage()), "\n"; }
        }
    }
} elseif ($case === 'wrapper-stat') {
    class InfoStatWrapper {
        public $context;
        function url_stat($path, $flags) {
            echo 'stat:', $path, ':', $flags, "\n";
            return ['mode'=>0100640, 'size'=>13, 'ino'=>23, 'uid'=>0, 'gid'=>0];
        }
    }
    stream_wrapper_register('infostat', InfoStatWrapper::class);
    $info = new SplFileInfo('infostat://item');
    var_dump($info->getSize(), $info->getInode(), $info->getType());
    clearstatcache();
    var_dump($info->isFile(), $info->isLink());
    clearstatcache();
    var_dump($info->isReadable());
} elseif ($case === 'local-file-wrapper') {
    foreach (['file://' . getcwd() . '/item.bin', 'FILE://localhost' . getcwd() . '/alias.bin', 'file://' . getcwd() . "/raw-\xff.\x80", 'file://remote/item.bin'] as $path) {
        $info = new SplFileInfo($path);
        var_dump($info->isReadable(), $info->isWritable(), $info->isExecutable());
        if (!str_contains($path, 'remote')) var_dump($info->getSize(), $info->isFile(), $info->getRealPath());
    }
} elseif ($case === 'inherited-string-contract') {
    class LabelInfo extends SplFileInfo { function __toString() { return 'label:' . $this->getFilename(); } }
    trait NumericInfoLabel { function __toString() { return 42; } }
    class NumericLabelInfo extends SplFileInfo { use NumericInfoLabel; }
    interface InfoLabelContract { function __toString(); }
    class NullLabelInfo extends SplFileInfo { function __toString() { return null; } }
    foreach ([LabelInfo::class, NumericInfoLabel::class, NumericLabelInfo::class, InfoLabelContract::class] as $class) {
        $method = new ReflectionMethod($class, '__toString');
        echo $class, ':', (string)$method->getReturnType(), ':', (int)$method->hasTentativeReturnType(), "\n";
    }
    foreach ([new LabelInfo('folder/item.bin'), new NumericLabelInfo('item.bin')] as $info) {
        echo (string)$info, ':', $info->__toString(), ':', '' . $info, "\n";
        echo $info->getPathname(), "\n";
    }
    attempt(fn() => (new NullLabelInfo('item.bin'))->__toString());
    attempt(fn() => (string)new NullLabelInfo('item.bin'));
} elseif ($case === 'reflection') {
    foreach (['__construct', 'getPath', 'getBasename', 'getPerms', 'getType', 'getFileInfo', 'getPathInfo', 'setInfoClass', 'setFileClass', '__toString', '__debugInfo', '_bad_state_ex'] as $name) {
        $method = new ReflectionMethod(SplFileInfo::class, $name);
        echo $name, ':', $method->getNumberOfRequiredParameters(), '/', $method->getNumberOfParameters(), ':', (string) $method->getReturnType(), ':', (string) $method->getTentativeReturnType(), ':', (int) $method->isFinal();
        foreach ($method->getParameters() as $parameter) { echo ':', $parameter->getName(), '/', (string) $parameter->getType(); if ($parameter->isDefaultValueAvailable()) echo '/', json_encode($parameter->getDefaultValue()); }
        echo "\n";
    }
}
