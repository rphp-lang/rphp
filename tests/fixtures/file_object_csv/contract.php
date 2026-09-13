<?php
// Original CSV record/control oracle; all native files belong to this process.
$files = [];
function csv_file($bytes, $mode = 'r') {
    global $files;
    $path = tempnam(sys_get_temp_dir(), 'rphp-csv-record-');
    $files[] = $path;
    file_put_contents($path, $bytes);
    return new SplFileObject($path, $mode);
}
function csv_text($message) {
    global $files;
    foreach ($files as $i => $path) $message = str_replace($path, '<file'.$i.'>', $message);
    return $message;
}
function csv_value($value) {
    if (is_string($value)) return 'hex:'.bin2hex($value);
    if (is_array($value)) { $out = []; foreach ($value as $key => $field) $out[$key] = csv_value($field); return $out; }
    return $value;
}
function show_csv($value) { echo json_encode(csv_value($value)), "\n"; }
function csv_attempt($action) {
    try { show_csv($action()); } catch (Throwable $e) { echo $e::class, ':', csv_text($e->getMessage()), "\n"; }
}
function csv_state($f) { echo 'state:', $f->key(), ':', (int)$f->eof(), ':', (int)$f->valid(), "\n"; }
set_error_handler(function($level, $message) { echo 'diag:', $level, ':', csv_text($message), "\n"; return true; });
try {
switch (getenv('RPHP_FILE_CSV_CASE')) {
case 'metadata':
    foreach (['fgetcsv','fputcsv','setCsvControl','getCsvControl'] as $name) {
        $r = new ReflectionMethod(SplFileObject::class, $name);
        echo $name, ':', $r->getNumberOfRequiredParameters(), '/', $r->getNumberOfParameters(), ':', $r->getTentativeReturnType(), "\n";
        foreach ($r->getParameters() as $p) {
            echo $p->getName(), ':', $p->getType(), ':', (int)$p->isOptional(), ':';
            show_csv($p->isDefaultValueAvailable() ? $p->getDefaultValue() : null);
        }
    }
    break;
case 'control-state':
    $f = csv_file("one;two\n"); show_csv($f->getCsvControl());
    show_csv($f->setCsvControl(';', "'", '')); show_csv($f->getCsvControl());
    show_csv($f->fgetcsv()); show_csv($f->getCsvControl());
    show_csv($f->setCsvControl()); show_csv($f->getCsvControl());
    break;
case 'control-errors':
    $f = csv_file("one;two\n"); $f->setCsvControl(';', '"', '');
    foreach ([[''],['::'],[',',''],[',','xx'],[',','"','xx'],[[]],[';',[]],[';','"',[]]] as $args) {
        csv_attempt(function() use ($f,$args) { return $f->setCsvControl(...$args); }); show_csv($f->getCsvControl());
    }
    show_csv($f->fgetcsv());
    break;
case 'read-records':
    $f = csv_file("a,b\r\n\n\"c,d\",\"e\"\"f\"\nlast,tail");
    for ($i = 0; $i < 6; ++$i) { show_csv($f->fgetcsv(escape: '')); csv_state($f); }
    break;
case 'multiline':
    $f = csv_file("\"north\nsouth\",end\r\n\"unfinished\nrecord");
    for ($i = 0; $i < 4; ++$i) { show_csv($f->fgetcsv(',', '"', '')); csv_state($f); }
    break;
case 'empty-eof':
    foreach (['',"\n",',',"\"\"\n","\n\n"] as $bytes) {
        show_csv($bytes); $f = csv_file($bytes);
        for ($i = 0; $i < 4; ++$i) { show_csv($f->fgetcsv(escape: '')); csv_state($f); }
    }
    break;
case 'iterator-flags':
    foreach ([8,9,10,12,14,15] as $flags) {
        echo 'flags:', $flags, "\n";
        $f = csv_file("\nleft,right\n\n\"two\nlines\",tail\n");
        $f->setCsvControl(',', '"', ''); $f->setFlags($flags); $f->rewind();
        for ($i = 0; $i < 7 && $f->valid(); ++$i) { echo 'key:', $f->key(), "\n"; show_csv($f->current()); $f->next(); }
        csv_state($f); show_csv($f->current());
    }
    break;
case 'cache-transitions':
    $f = csv_file("a;b\nc;d\ne;f\n");
    $f->setCsvControl(';', '"', ''); show_csv($f->current());
    $f->setFlags(8); show_csv($f->current()); show_csv($f->fgetcsv());
    show_csv($f->current()); csv_state($f); $f->next(); show_csv($f->current());
    $f->setCsvControl(',', '"', ''); show_csv($f->current());
    $f->rewind(); show_csv($f->current()); csv_state($f);
    break;
case 'seek-length':
    $f = csv_file("\"a\nb\",c\nx,y\n"); $f->setCsvControl(',', '"', ''); $f->setFlags(8);
    foreach ([0,1,2,0] as $line) { $f->seek($line); show_csv($f->current()); csv_state($f); }
    $f->rewind(); $f->setMaxLineLen(3);
    for ($i = 0; $i < 5; ++$i) { show_csv($f->fgetcsv()); csv_state($f); }
    break;
case 'binary-controls':
    $f = csv_file("a\xffb\n\"\x80\0\",z\n");
    $f->setCsvControl("\xff", '"', ''); show_csv($f->getCsvControl()); show_csv($f->fgetcsv());
    show_csv($f->fgetcsv(',', '"', '')); show_csv($f->getCsvControl());
    break;
case 'default-diagnostics':
    $f = csv_file("a,b\nc,d\ne,f\n");
    show_csv($f->fgetcsv()); $f->setCsvControl(';'); show_csv($f->fgetcsv());
    $f->setCsvControl(',', '"', '\\'); show_csv($f->fgetcsv());
    $f->rewind(); $f->setFlags(8); show_csv($f->current());
    break;
case 'diagnostic-exception':
    $f = csv_file("a,b\nc,d\n");
    set_error_handler(function($level, $message) { throw new RuntimeException('blocked escape'); });
    csv_attempt(function() use ($f) { return $f->fgetcsv(); });
    restore_error_handler(); csv_state($f); show_csv($f->fgetcsv(escape: ''));
    break;
case 'argument-order':
    $f = csv_file("a,b\nc,d\n");
    foreach ([['::'],[',','::'],[',','"','xx'],[null], [1,2,3]] as $args) {
        csv_attempt(function() use ($f,$args) { return $f->fgetcsv(...$args); }); csv_state($f);
    }
    show_csv($f->fgetcsv(escape: ''));
    break;
case 'strict-arguments':
    $f = csv_file("a,b\n", 'r+');
    foreach ([1,null,[],false] as $argument) {
        csv_attempt(function() use ($f,$argument) { return eval('declare(strict_types=1); return $f->fgetcsv($argument);'); });
        csv_attempt(function() use ($f,$argument) { return eval('declare(strict_types=1); return $f->setCsvControl($argument);'); });
    }
    show_csv($f->fgetcsv(escape: ''));
    break;
case 'write-records':
    $f = csv_file('', 'r+');
    foreach ([[],['plain','a b',"tab\tfield"],['a,b','q"q',"a\nb"],[null,true,false,17,-2.5],["\xff\0",'tail']] as $fields) {
        show_csv($f->fputcsv($fields, escape: '')); csv_state($f);
    }
    show_csv(file_get_contents($files[0]));
    $f->rewind(); for ($i = 0; $i < 6; ++$i) show_csv($f->fgetcsv(escape: ''));
    break;
case 'write-controls':
    $f = csv_file('', 'r+'); $f->setCsvControl(';', "'", '');
    show_csv($f->fputcsv(['a;b',"q'q"])); show_csv($f->getCsvControl());
    show_csv($f->fputcsv(fields: ['x,y','z'], separator: ',', enclosure: '"', escape: '', eol: "\r\n"));
    show_csv($f->getCsvControl()); show_csv(file_get_contents($files[0]));
    break;
case 'write-errors':
    $f = csv_file('', 'r+');
    foreach ([[17], [[], '::'], [[], ',', 'xx'], [[], ',', '"', 'xx'], [[], ',', '"', '', []]] as $args) {
        csv_attempt(function() use ($f,$args) { return $f->fputcsv(...$args); });
    }
    show_csv(file_get_contents($files[0]));
    $r = csv_file("untouched\n"); csv_attempt(function() use ($r) { return $r->fputcsv(['new'], escape: ''); });
    show_csv(file_get_contents($files[1]));
    break;
case 'write-field-conversion':
    class CsvField { public function __toString(): string { echo "stringified\n"; return 'value,quoted'; } }
    $f = csv_file('', 'r+'); $field = 'aliased'; $values = [&$field, new CsvField, ['nested']];
    show_csv($f->fputcsv($values, escape: '')); show_csv($field); show_csv(file_get_contents($files[0]));
    set_error_handler(function($level,$message) { throw new RuntimeException('field warning'); });
    csv_attempt(function() use ($f) { return $f->fputcsv(['prefix', []], escape: ''); });
    restore_error_handler(); show_csv(file_get_contents($files[0]));
    break;
case 'deprecation-reentry':
    $f = csv_file("left;right\nlast;field\n");
    set_error_handler(function($level, $message) use ($f) {
        echo 'callback:', csv_text($message), "\n";
        $f->setCsvControl(';', "'", ''); return true;
    });
    show_csv($f->fgetcsv()); restore_error_handler();
    show_csv($f->getCsvControl()); show_csv($f->fgetcsv());
    break;
case 'string-projection':
    foreach (['current','string','line','direct'] as $start) {
        echo $start, "\n"; $f = csv_file("\"a\nb\",c\nx,y\n");
        $f->setFlags(8); $f->setCsvControl(',', '"', '');
        if ($start === 'current') show_csv($f->current());
        if ($start === 'string') show_csv((string)$f);
        if ($start === 'line') show_csv($f->getCurrentLine());
        if ($start === 'direct') show_csv($f->fgetcsv());
        show_csv((string)$f); show_csv($f->current()); csv_state($f);
        show_csv($f->getCurrentLine()); show_csv((string)$f); show_csv($f->current()); csv_state($f);
    }
    break;
case 'mixed-error-priority':
    $f = csv_file('', 'r+');
    foreach ([[[], 'xx', '"', '', []], [[], 'xx', [], '', "\n"], [[], ',', 'xx', [], "\n"]] as $args) {
        csv_attempt(function() use ($f,$args) { return $f->fputcsv(...$args); });
    }
    show_csv($f->getCsvControl()); show_csv(file_get_contents($files[0]));
    break;
case 'write-only-read':
    $f = csv_file('', 'w');
    for ($i = 0; $i < 2; ++$i) { csv_attempt(function() use ($f) { return $f->fgetcsv(escape: ''); }); csv_state($f); }
    break;
case 'memory-write-permissions':
    foreach (['php://memory','php://temp'] as $path) {
        $f = new SplFileObject($path, 'r');
        csv_attempt(function() use ($f) { return $f->fputcsv(['kept','apart'], escape: ''); }); csv_state($f);
        show_csv($f->fgetcsv(escape: ''));
    }
    break;
case 'empty-record-write':
    $f = csv_file("untouched\n");
    show_csv($f->fputcsv([], escape: '', eol: '')); csv_state($f);
    show_csv(file_get_contents($files[0]));
    $f = new SplFileObject('php://memory', 'r');
    show_csv($f->fputcsv([], escape: '', eol: '')); csv_state($f);
    break;
case 'write-reentry':
    class ReenterCsvField {
        public $file;
        public $fields;
        public function __toString(): string {
            echo "convert\n";
            $this->fields[] = 'new';
            $this->file->setCsvControl(';', "'", '');
            show_csv($this->file->fputcsv(['nested','write']));
            return 'outer,field';
        }
    }
    $f = csv_file('', 'r+'); $o = new ReenterCsvField; $o->file = $f;
    $fields = ['before', $o, 'after']; $o->fields = &$fields;
    show_csv($f->fputcsv($fields, escape: ''));
    show_csv(count($fields)); show_csv(file_get_contents($files[0]));
    unset($o->fields, $o->file, $fields, $o, $f); gc_collect_cycles();
    break;
case 'wrapper-partial-write':
    class PartialCsvWrapper {
        public $context;
        public static $count;
        public function url_stat($path,$flags) { return ['mode'=>0100000]; }
        public function stream_open($path,$mode,$options,&$opened) { return true; }
        public function stream_write($bytes) { show_csv($bytes); return self::$count; }
        public function stream_flush() { echo "flush\n"; return true; }
        public function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('partialcsv', PartialCsvWrapper::class);
    foreach ([2,0,false,-1,100] as $index => $count) {
        PartialCsvWrapper::$count = $count;
        $f = new SplFileObject('partialcsv://record'.$index, 'r+');
        csv_attempt(function() use ($f) { return $f->fputcsv(['abc','def'], escape: ''); }); unset($f);
    }
    stream_wrapper_unregister('partialcsv');
    break;
case 'cache-cow':
    $f = csv_file("first,second\nlast,field\n"); $f->setFlags(8); $f->setCsvControl(',', '"', '');
    $first = $f->current(); $copy = $first; $first[0] = 'changed'; $copy[1] = $f;
    show_csv($first); show_csv($f->current()); show_csv((string)$f);
    unset($first, $copy); gc_collect_cycles();
    $f->next(); show_csv($f->current()); csv_state($f);
    break;
case 'wrapper-byte-records':
    class ByteCsvWrapper {
        public $context;
        public array $items = ["\u{00e9},\u{03bb}\n", "\xff,\0\n", "last,field\n"];
        public int $index = 0;
        public function url_stat($path,$flags) { return ['mode'=>0100000]; }
        public function stream_open($path,$mode,$options,&$opened) { return true; }
        public function stream_read($count) { return $this->items[$this->index++] ?? ''; }
        public function stream_eof() { return $this->index >= count($this->items); }
        public function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('bytecsv', ByteCsvWrapper::class);
    $f = new SplFileObject('bytecsv://record');
    for ($i = 0; $i < 3; ++$i) show_csv($f->fgetcsv(escape: ''));
    unset($f); stream_wrapper_unregister('bytecsv');
    break;
case 'wrapper-write-exception':
    class ThrowCsvWrapper {
        public $context;
        public static $mode;
        public function url_stat($path,$flags) { return ['mode'=>0100000]; }
        public function stream_open($path,$mode,$options,&$opened) { return true; }
        public function stream_write($bytes) { echo "write\n"; if (self::$mode === 'write') throw new RuntimeException('writing'); return strlen($bytes); }
        public function stream_flush() { echo "flush\n"; if (self::$mode === 'flush') throw new RuntimeException('flushing'); return true; }
        public function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('throwcsv', ThrowCsvWrapper::class);
    foreach (['write','flush'] as $index => $mode) {
        ThrowCsvWrapper::$mode = $mode;
        $f = new SplFileObject('throwcsv://record'.$index, 'r+');
        csv_attempt(function() use ($f) { return $f->fputcsv(['a'], escape: ''); });
        try { unset($f); } catch (Throwable $e) { echo $e::class, ':', $e->getMessage(), "\n"; }
    }
    stream_wrapper_unregister('throwcsv');
    break;
case 'wrapper-interrupted-write':
    class SequenceCsvWrapper {
        public $context;
        public static array $returns;
        public function url_stat($path,$flags) { return ['mode'=>0100000]; }
        public function stream_open($path,$mode,$options,&$opened) { return true; }
        public function stream_write($bytes) {
            show_csv($bytes); $result = array_shift(self::$returns);
            if ($result === 'throw') throw new RuntimeException('write failed');
            return $result;
        }
        public function stream_flush() { echo "flush\n"; return true; }
        public function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('sequencecsv', SequenceCsvWrapper::class);
    foreach ([[2,false],[2,0],[2,-1],[2,'throw']] as $index => $returns) {
        SequenceCsvWrapper::$returns = $returns;
        $f = new SplFileObject('sequencecsv://record'.$index, 'r+');
        csv_attempt(function() use ($f) { return $f->fputcsv(['abc','def'], escape: ''); }); unset($f);
    }
    SequenceCsvWrapper::$returns = [20]; $f = new SplFileObject('sequencecsv://array', 'r+');
    set_error_handler(function() { throw new RuntimeException('field failed'); });
    csv_attempt(function() use ($f) { return $f->fputcsv([[]], escape: ''); });
    restore_error_handler(); unset($f); stream_wrapper_unregister('sequencecsv');
    break;
case 'wrapper-record-chunks':
    class ChunkCsvWrapper {
        public $context;
        public function url_stat($path,$flags) { return ['mode'=>0100000]; }
        public function stream_open($path,$mode,$options,&$opened) { return true; }
        public function stream_write($bytes) { echo 'chunk:', strlen($bytes), "\n"; return strlen($bytes); }
        public function stream_flush() { echo "flush\n"; return true; }
        public function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('chunkcsv', ChunkCsvWrapper::class);
    foreach ([8191,8192,8193,20001] as $length) {
        $f = new SplFileObject('chunkcsv://'.$length, 'r+');
        show_csv($f->fputcsv([str_repeat('x', $length)], escape: '')); unset($f);
    }
    stream_wrapper_unregister('chunkcsv');
    break;
case 'wrapper-read':
case 'wrapper-write':
case 'wrapper-failure':
    class CsvWrapper {
        public $context;
        public static string $bytes = "\"one\ntwo\",end\nlast,field\n";
        public static bool $fail = false;
        public int $position = 0;
        public function url_stat($path,$flags) { echo 'stat:', $flags, "\n"; return ['mode'=>0100000,'size'=>strlen(self::$bytes)]; }
        public function stream_open($path,$mode,$options,&$opened) { echo 'open:', $mode, ':', $options, "\n"; return true; }
        public function stream_read($count) {
            echo 'read:', $count, ':', $this->position, "\n";
            if (self::$fail) { self::$fail = false; throw new RuntimeException('read failed'); }
            $bytes = substr(self::$bytes, $this->position, 3); $this->position += strlen($bytes); return $bytes;
        }
        public function stream_write($bytes) {
            echo 'write:', bin2hex($bytes), "\n";
            self::$bytes = substr(self::$bytes, 0, $this->position).$bytes.substr(self::$bytes, $this->position+strlen($bytes));
            $this->position += strlen($bytes); return strlen($bytes);
        }
        public function stream_eof() { echo 'eof:', $this->position, "\n"; return $this->position >= strlen(self::$bytes); }
        public function stream_seek($offset,$whence) { echo 'seek:', $offset, ':', $whence, "\n"; $this->position = $offset; return true; }
        public function stream_tell() { return $this->position; }
        public function stream_flush() { echo "flush\n"; return true; }
        public function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('csvsample', CsvWrapper::class);
    $f = new SplFileObject('csvsample://record', 'r+'); $f->setCsvControl(',', '"', '');
    if (getenv('RPHP_FILE_CSV_CASE') === 'wrapper-write') {
        show_csv($f->fputcsv(['a,b',"two\nlines"], escape: '')); show_csv(CsvWrapper::$bytes);
    } else {
        if (getenv('RPHP_FILE_CSV_CASE') === 'wrapper-failure') CsvWrapper::$fail = true;
        for ($i = 0; $i < 3; ++$i) { csv_attempt(function() use ($f) { return $f->fgetcsv(); }); csv_state($f); }
        $f->rewind(); show_csv($f->fgetcsv()); csv_state($f);
    }
    unset($f); stream_wrapper_unregister('csvsample');
    break;
default: throw new RuntimeException('unknown case');
}
} finally {
    restore_error_handler();
    foreach ($files as $path) if (file_exists($path)) unlink($path);
}
