<?php
// Original physical stream/iterator contract. Files belong only to this run.
$read_files = [];
function read_file($bytes, $mode = 'r') {
    global $read_files;
    $path = tempnam(sys_get_temp_dir(), 'rphp-file-read-');
    $read_files[] = $path;
    file_put_contents($path, $bytes);
    return new SplFileObject($path, $mode);
}
function read_message($text) {
    global $read_files;
    foreach ($read_files as $i => $path) $text = str_replace($path, '<file'.$i.'>', $text);
    return $text;
}
function read_value($value) {
    if (is_string($value)) return 'hex:'.bin2hex($value);
    if (is_array($value)) { $result = []; foreach ($value as $k => $v) $result[$k] = read_value($v); return $result; }
    return $value;
}
function read_show($value) { echo json_encode(read_value($value)), "\n"; }
function read_attempt($action) {
    try { read_show($action()); }
    catch (Throwable $e) { echo $e::class, ':', read_message($e->getMessage()), "\n"; }
}
function read_state($file) { echo 'state:', $file->ftell(), ':', $file->key(), ':', (int)$file->eof(), "\n"; }
set_error_handler(function($level, $message) { echo 'diag:', $level, ':', read_message($message), "\n"; return true; });
try {
switch (getenv('RPHP_FILE_READ_CASE')) {
case 'metadata':
    foreach (['ftell','fgets','fread','fgetc'] as $name) {
        $method = new ReflectionMethod(SplFileObject::class, $name);
        echo $name, ':', $method->getNumberOfRequiredParameters(), '/', $method->getNumberOfParameters(), ':', $method->getTentativeReturnType(), "\n";
        foreach ($method->getParameters() as $p) echo $p->getName(), ':', $p->getType(), ':', (int)$p->isOptional(), "\n";
    }
    break;
case 'tell':
    $file = read_file("north\nsouth\ntail"); $alias = $file;
    read_state($file); read_state($alias);
    read_show($file->current()); read_state($alias);
    read_show($file->current()); read_state($alias);
    $file->next(); read_state($alias);
    read_show($file->current()); read_state($alias);
    $file->rewind(); read_state($alias);
    $file->seek(2); read_state($alias); read_show($file->current()); read_state($alias);
    $file->seek(0); read_state($alias);
    break;
case 'bytes':
    foreach (["a\nb\r\n\x00\xffZ", '', "\n"] as $bytes) {
        $file = read_file($bytes); read_state($file);
        for ($i = 0; $i < strlen($bytes) + 2; ++$i) { read_show($file->fgetc()); read_state($file); }
    }
    break;
case 'chunks':
    $file = read_file("ab\r\ncd\n\xc3\xa9\xffZ"); $alias = $file;
    foreach ([1,3,2,1,40,2] as $length) { read_show($file->fread($length)); read_state($alias); }
    $file->rewind(); read_show($file->fread(length: 2)); read_state($alias);
    break;
case 'lines':
    foreach ([0,1,2,3,4,5,6,7,8,15] as $flags) {
        echo 'flags:', $flags, "\n";
        $file = read_file("\nfirst\r\n\nlast"); $file->setCsvControl(',', '"', ''); $file->setFlags($flags);
        for ($i = 0; $i < 6; ++$i) { read_attempt(fn() => $file->fgets()); read_state($file); }
    }
    break;
case 'lengths':
    foreach ([0,1,3,5] as $limit) {
        echo 'limit:', $limit, "\n";
        $file = read_file("abcd\r\nq\rtail\n"); $file->setMaxLineLen($limit); $file->setFlags(SplFileObject::DROP_NEW_LINE);
        for ($i = 0; $i < 7; ++$i) { read_attempt(fn() => $file->fgets()); read_state($file); }
    }
    break;
case 'cache':
    foreach ([0,2] as $flags) {
        echo 'flags:', $flags, "\n";
        $file = read_file("alpha\nbeta\ngamma\nend"); $file->setFlags($flags); $file->rewind();
        read_show($file->current()); read_state($file);
        read_show($file->fread(2)); read_state($file); read_show($file->current());
        read_show($file->fgetc()); read_state($file); read_show($file->current());
        read_attempt(fn() => $file->fgets()); read_state($file); read_show($file->current());
        $file->seek(1); read_state($file); read_attempt(fn() => $file->fgets()); read_state($file);
        $file->rewind(); read_show($file->fread(1)); read_show($file->current()); read_state($file);
    }
    break;
case 'csv':
    $file = read_file("\"a\nb\",c\nx,y\nlast,end"); $file->setCsvControl(',', '"', ''); $file->setFlags(8);
    read_show($file->current()); read_state($file);
    read_show($file->fgetc()); read_state($file); read_show($file->current());
    read_attempt(fn() => $file->fgets()); read_state($file); read_show($file->current());
    $file->rewind(); read_show($file->fread(2)); read_state($file); read_show($file->fgetcsv()); read_state($file);
    break;
case 'arguments':
    $file = read_file('0123456789');
    foreach ([0,-1,null,false,true,'2','wrong',2.5,[],new stdClass()] as $length) {
        read_attempt(fn() => $file->fread($length)); read_state($file);
    }
    foreach (['ftell','fgets','fgetc'] as $method) { read_attempt(fn() => $file->$method(1)); read_state($file); }
    read_attempt(fn() => $file->fread());
    read_attempt(fn() => $file->fread(length: 1, extra: 2));
    read_attempt(fn() => $file->fread(amount: 1));
    read_state($file);
    break;
case 'strict':
    $file = read_file('abcdef');
    $strict = eval('declare(strict_types=1); return function($f, $n) { return $f->fread($n); };');
    foreach (['2',2.0,true,null,2] as $length) { read_attempt(fn() => $strict($file, $length)); read_state($file); }
    break;
case 'reentry':
    $file = read_file("abcdefgh\n"); $alias = $file;
    set_error_handler(function($level, $message) use ($alias) { echo 'callback:', $level, ':', read_message($message), "\n"; read_show($alias->fgetc()); read_state($alias); return true; });
    read_show($file->fread(2.5)); read_state($file);
    set_error_handler(function($level, $message) use ($alias) { read_show($alias->fgetc()); throw new Exception('stop-read'); });
    read_attempt(fn() => $file->fread(2.5)); read_state($file);
    restore_error_handler(); restore_error_handler();
    read_show($file->fread(2)); read_state($file);
    break;
case 'permissions':
    foreach (['fgetc','fgets','fread'] as $method) {
        $file = read_file('old', 'w');
        read_attempt(fn() => $method === 'fread' ? $file->fread(3) : $file->$method()); read_state($file);
    }
    break;
case 'cache_eof':
    foreach (['', 'tail', "tail\n"] as $bytes) {
        $file = read_file($bytes); read_show($file->current()); read_state($file);
        read_attempt(fn() => $file->fgets()); read_state($file); read_show($file->current());
        read_attempt(fn() => $file->fgets()); read_state($file); read_show($file->current());
    }
    break;
case 'overrides':
    class ReadOverride extends SplFileObject {
        function getCurrentLine(): string { echo "override\n"; return 'override:'.parent::fgets(); }
    }
    $base = read_file("a\nb\nc\nd\n");
    $file = new ReadOverride($read_files[0]);
    read_show($file->current()); read_state($file);
    read_show($file->fgets()); read_state($file); read_show($file->current());
    read_show($file->fgetc()); read_state($file); read_show($file->current());
    class ReadUninitialized extends SplFileObject { function __construct() {} }
    $file = new ReadUninitialized();
    foreach (['fgets','fgetc','ftell'] as $method) read_attempt(fn() => $file->$method());
    foreach ([0,1,[]] as $n) read_attempt(fn() => $file->fread($n));
    break;
case 'wrapper_seek':
    class SeekReadStream {
        public $context;
        public static $position = 0;
        public static $accept = true;
        function url_stat($p,$f) { return ['mode'=>0100644]; }
        function stream_open($p,$m,$o,&$x) { return true; }
        function stream_seek($o,$w) { echo "seek:$o:$w\n"; return self::$accept; }
        function stream_tell() { echo "tell\n"; return self::$position; }
        function stream_read($n) { echo "read:$n\n"; return 'X'; }
        function stream_eof() { return false; }
    }
    stream_wrapper_register('readseek', SeekReadStream::class);
    foreach ([0,4,-1,-2,false,null,'3'] as $n) {
        SeekReadStream::$position = $n; read_show($n);
        $file = new SplFileObject('readseek://record');
        read_attempt(fn() => $file->rewind()); read_state($file);
        read_show($file->fgetc()); read_state($file); unset($file);
    }
    SeekReadStream::$accept = false;
    $file = new SplFileObject('readseek://record');
    read_attempt(fn() => $file->rewind()); read_state($file); unset($file);
    stream_wrapper_unregister('readseek');
    class NoTellReadStream {
        public $context;
        function url_stat($p,$f) { return ['mode'=>0100644]; }
        function stream_open($p,$m,$o,&$x) { return true; }
        function stream_seek($o,$w) { return true; }
    }
    stream_wrapper_register('readnotell', NoTellReadStream::class);
    $file = new SplFileObject('readnotell://record');
    read_attempt(fn() => $file->rewind()); read_show($file->ftell());
    read_attempt(fn() => $file->seek(0)); read_show($file->ftell());
    unset($file); stream_wrapper_unregister('readnotell');
    break;
case 'wrapper_failure':
    class FaultReadStream {
        public $context;
        public static $fail = '';
        private $position = 0;
        function url_stat($p,$f) { return ['mode'=>0100644]; }
        function stream_open($p,$m,$o,&$x) { return true; }
        function stream_read($n) {
            echo "read:$n\n";
            if (self::$fail === 'read') throw new Exception('read failed');
            ++$this->position; return 'K';
        }
        function stream_eof() { echo "eof\n"; if (self::$fail === 'eof') throw new Exception('eof failed'); return false; }
        function stream_seek($o,$w) { echo "seek\n"; if (self::$fail === 'seek') throw new Exception('seek failed'); $this->position = 0; return true; }
        function stream_tell() { echo "tell\n"; if (self::$fail === 'tell') throw new Exception('tell failed'); return $this->position; }
        function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('readfault', FaultReadStream::class);
    foreach (['read','eof','seek','tell'] as $failure) {
        echo "failure:$failure\n"; $file = new SplFileObject('readfault://record');
        FaultReadStream::$fail = $failure;
        read_attempt(fn() => in_array($failure,['seek','tell']) ? $file->rewind() : $file->fread(2));
        FaultReadStream::$fail = ''; read_state($file);
        read_show($file->fgetc()); read_state($file); unset($file);
    }
    stream_wrapper_unregister('readfault');
    break;
case 'wrapper_short':
    class ShortReadStream {
        public $context;
        public static $end = false;
        function url_stat($p,$f) { return ['mode'=>0100644]; }
        function stream_open($p,$m,$o,&$x) { return true; }
        function stream_read($n) { echo "read:$n\n"; return 'K'; }
        function stream_eof() { echo "eof\n"; return self::$end; }
    }
    stream_wrapper_register('readshort', ShortReadStream::class);
    $file = new SplFileObject('readshort://record');
    foreach ([false,true,false] as $end) {
        ShortReadStream::$end = $end;
        read_show($file->fread(3)); read_state($file);
    }
    unset($file); stream_wrapper_unregister('readshort');
    break;
case 'wrapper_empty':
    class EmptyReadStream {
        public $context;
        function url_stat($p,$f) { return ['mode'=>0100644]; }
        function stream_open($p,$m,$o,&$x) { return true; }
        function stream_read($n) { echo "read:$n\n"; return ''; }
        function stream_eof() { echo "eof\n"; return true; }
    }
    stream_wrapper_register('readempty', EmptyReadStream::class);
    foreach (['fgets','fgetc','fread'] as $method) {
        $file = new SplFileObject('readempty://record');
        echo $method,"\n";
        read_attempt(fn() => $method === 'fread' ? $file->fread(2) : $file->$method());
        read_state($file); unset($file);
    }
    stream_wrapper_unregister('readempty');
    break;
case 'wrapper_reentry':
    class ReenterReadStream {
        public $context;
        public static $file;
        private $sent = false;
        function url_stat($p,$f) { return ['mode'=>0100644]; }
        function stream_open($p,$m,$o,&$x) { return true; }
        function stream_read($n) {
            echo "read:$n\n";
            if ($this->sent) return '';
            $this->sent = true;
            self::$file->setFlags(SplFileObject::DROP_NEW_LINE);
            echo 'inner-position:',self::$file->ftell(),"\n";
            stream_wrapper_unregister('readenter'); self::$file = null;
            gc_collect_cycles();
            return "A\r\nB\n";
        }
        function stream_eof() { echo "eof\n"; return $this->sent; }
        function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('readenter', ReenterReadStream::class);
    $file = new SplFileObject('readenter://record'); $alias = $file;
    ReenterReadStream::$file = $file;
    read_show($file->fgets()); read_state($file);
    unset($file); read_show($alias->fgetc()); read_state($alias);
    read_show($alias->fread(8)); read_state($alias); unset($alias);
    gc_collect_cycles();
    break;
case 'wrapper':
    class ReadStream {
        public $context;
        public static $events = [];
        private $position = 0;
        private $bytes = "one\ntwo\n\xfftail";
        function url_stat($path, $flags) { self::$events[] = ['stat',$flags]; return ['mode'=>0100644,'size'=>14]; }
        function stream_open($path, $mode, $options, &$opened) { self::$events[] = ['open',$options]; return true; }
        function stream_read($length) { self::$events[] = ['read',$length]; $value = substr($this->bytes,$this->position,$length); $this->position += strlen($value); return $value; }
        function stream_eof() { self::$events[] = ['eof']; return $this->position >= strlen($this->bytes); }
        function stream_tell() { self::$events[] = ['tell']; return $this->position; }
        function stream_seek($offset, $whence) { self::$events[] = ['seek',$offset,$whence]; $this->position = $offset; return true; }
        function stream_close() { self::$events[] = ['close']; }
    }
    stream_wrapper_register('readcase', ReadStream::class);
    $file = new SplFileObject('readcase://record');
    read_state($file); read_show($file->fgetc()); read_state($file);
    read_show($file->fread(2)); read_state($file); read_show($file->fgets()); read_state($file);
    $file->rewind(); read_show($file->fgets()); read_show($file->fread(100)); read_state($file);
    read_show($file->fgetc()); read_state($file); unset($file);
    read_show(ReadStream::$events); stream_wrapper_unregister('readcase');
    break;
default: throw new Exception('Unknown original read case');
}
} finally {
    restore_error_handler();
    foreach ($read_files as $path) unlink($path);
}
