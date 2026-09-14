<?php
// Original temporary-file ownership and shared stream mutation contracts.
$temp_paths = [];
function temp_normalize($text) {
    global $temp_paths;
    foreach ($temp_paths as $i => $path) $text = str_replace($path, '<file'.$i.'>', $text);
    return $text;
}
function temp_show($value) { echo temp_normalize(json_encode($value, JSON_INVALID_UTF8_SUBSTITUTE)), "\n"; }
function temp_attempt($action) {
    try { temp_show($action()); }
    catch (Throwable $e) { echo $e::class, ':', temp_normalize($e->getMessage()), "\n"; }
}
function temp_state($f) { temp_show([$f->ftell(), $f->key(), $f->eof()]); }
function temp_native($mode = 'r+') {
    global $temp_paths;
    $path = tempnam(sys_get_temp_dir(), 'rphp-temp-contract-');
    $temp_paths[] = $path;
    file_put_contents($path, "first\nsecond\n");
    return new SplFileObject($path, $mode);
}
set_error_handler(function($level, $message) { echo 'diagnostic:', $level, ':', temp_normalize($message), "\n"; return true; });
try {
switch (getenv('RPHP_TEMP_FILE_CASE')) {
case 'metadata':
    foreach (['SplTempFileObject'=>['__construct'], 'SplFileObject'=>['fwrite','fflush','ftruncate','flock','fstat']] as $class=>$names) {
        foreach ($names as $name) {
            $m = new ReflectionMethod($class, $name);
            echo $class, '::', $name, ':', $m->getNumberOfRequiredParameters(), '/', $m->getNumberOfParameters(), ':', $m->getReturnType(), ':', $m->getTentativeReturnType(), "\n";
            foreach ($m->getParameters() as $p) {
                echo $p->getName(), ':', $p->getType(), ':', (int)$p->isPassedByReference(), ':';
                temp_show($p->isDefaultValueAvailable() ? $p->getDefaultValue() : 'required');
            }
        }
    }
    $f = new SplTempFileObject();
    temp_show([$f instanceof SplFileObject, $f instanceof SplFileInfo, $f instanceof RecursiveIterator, $f instanceof SeekableIterator]);
    break;
case 'inherited_metadata':
    foreach ([SplFileInfo::class,SplFileObject::class,SplTempFileObject::class] as $class) {
        foreach (['__construct','__debugInfo','getPathInfo'] as $name) {
            $method = new ReflectionMethod($class,$name);
            temp_show([$class,$name,$method->getDeclaringClass()->getName()]);
        }
    }
    $f = new SplTempFileObject(); temp_attempt(fn()=>$f->__debugInfo(1));
    break;
case 'paths':
    foreach ([null,-7,0,1,32,2097152] as $limit) {
        $f = $limit === null ? new SplTempFileObject() : new SplTempFileObject(maxMemory:$limit);
        temp_show([$f->getPath(),$f->getFilename(),$f->getPathname(),$f->getBasename(),$f->getExtension(),$f->getRealPath()]);
        temp_show($f->__debugInfo());
        $f->setCsvControl(';', "'", ''); temp_show($f->__debugInfo());
    }
    break;
case 'constructor':
    foreach ([true,false,null,'4',4.5,[],new stdClass(),'wrong'] as $limit) {
        temp_attempt(fn()=>(new SplTempFileObject($limit))->getPathname());
    }
    $f = new SplTempFileObject(3); $f->fwrite('kept');
    temp_attempt(fn()=>$f->__construct()); temp_attempt(fn()=>$f->__construct([]));
    temp_state($f); temp_show($f->getPathname());
    temp_attempt(fn()=>new SplTempFileObject(1,2));
    temp_attempt(fn()=>new SplTempFileObject(limit:1));
    break;
case 'strict':
    $ctor = eval('declare(strict_types=1); return function($n) { return new SplTempFileObject($n); };');
    foreach (['1',1.0,true,null,1] as $n) temp_attempt(fn()=>$ctor($n)->getPathname());
    $f = new SplTempFileObject();
    $write = eval('declare(strict_types=1); return function($f,$s,$n) { return $f->fwrite($s,$n); };');
    foreach ([[5,1],['abc','1'],['abc',1.0],['abc',null],['abc',1]] as $pair) { temp_attempt(fn()=>$write($f,...$pair)); temp_state($f); }
    break;
case 'write':
    foreach ([-1,0,2,128] as $limit) {
        $f = new SplTempFileObject($limit);
        foreach ([null,0,1,100,-1,-4] as $length) {
            temp_attempt(fn()=>$f->fwrite(data:"A\x00\xffZ",length:$length)); temp_state($f);
        }
        $f->rewind(); temp_show(bin2hex($f->fread(100))); temp_state($f);
    }
    break;
case 'cache':
    foreach ([-1,0,64] as $limit) {
        foreach ([0,2,8] as $flags) {
            $f = new SplTempFileObject($limit); $alias = $f;
            $f->setCsvControl(',', '"', ''); $f->setFlags($flags);
            $f->fwrite("alpha,beta\nlast,end\n"); $f->rewind(); temp_show($f->current()); temp_state($f);
            temp_show($f->fwrite('XY')); temp_show($alias->current()); temp_state($f);
            temp_show($f->ftruncate(3)); temp_show($alias->current()); temp_state($f);
            $f->rewind(); temp_show($f->current()); temp_state($f);
        }
    }
    break;
case 'truncate':
    foreach ([-1,0,64] as $limit) {
        $f = new SplTempFileObject($limit); $f->fwrite('abcdef');
        temp_show($f->ftruncate(2)); temp_state($f);
        temp_show($f->fwrite('XY')); temp_state($f);
        $f->rewind(); temp_show(bin2hex($f->fread(30))); temp_state($f);
        temp_show($f->ftruncate(12)); temp_state($f);
        $f->rewind(); temp_show(bin2hex($f->fread(30))); temp_state($f);
        temp_attempt(fn()=>$f->ftruncate(-1)); temp_state($f);
        temp_show($f->ftruncate(0)); temp_show($f->fflush()); temp_state($f);
    }
    break;
case 'eof':
    foreach ([-1,0,64] as $limit) {
        $f = new SplTempFileObject($limit); temp_show($f->fread(1)); temp_state($f);
        temp_show($f->fwrite('new')); temp_state($f); temp_show($f->current()); temp_state($f);
        temp_show($f->ftruncate(9)); temp_state($f); temp_show($f->fflush()); temp_state($f);
        $f->rewind(); temp_show(bin2hex($f->fread(20))); temp_state($f);
    }
    break;
case 'arguments':
    $f = new SplTempFileObject();
    foreach ([null,false,12,1.5,[],new stdClass()] as $data) { temp_attempt(fn()=>$f->fwrite($data)); temp_state($f); }
    foreach ([false,true,'2',2.5,'bad',[],new stdClass()] as $n) { temp_attempt(fn()=>$f->fwrite('abc',$n)); temp_state($f); }
    foreach ([false,null,'3',3.5,'bad',[]] as $n) { temp_attempt(fn()=>$f->ftruncate($n)); temp_state($f); }
    temp_attempt(fn()=>$f->fwrite()); temp_attempt(fn()=>$f->ftruncate());
    temp_attempt(fn()=>$f->fflush(1)); temp_attempt(fn()=>$f->fstat(1));
    break;
case 'reentry':
    $f = new SplTempFileObject(); $alias = $f;
    set_error_handler(function($level,$message) use($alias) { echo 'callback:', $level, ':', $message, "\n"; $alias->fwrite('!'); return true; });
    temp_show($f->fwrite('abcd',2.5)); temp_state($f);
    set_error_handler(function($level,$message) use($alias) { $alias->fwrite('?'); throw new Exception('conversion stopped'); });
    temp_attempt(fn()=>$f->ftruncate(1.5)); temp_state($f);
    restore_error_handler(); restore_error_handler();
    $f->rewind(); temp_show($f->fread(30));
    break;
case 'stat_lock':
    foreach ([-1,0,8] as $limit) {
        $f = new SplTempFileObject($limit); $f->fwrite('123456789');
        $stat = $f->fstat(); temp_show([count($stat),$stat['size'],$stat[7],$stat['size']===$stat[7]]);
        foreach ([0,1,2,3,4,6,19] as $operation) {
            $blocked = 'initial'; temp_attempt(function() use($f,$operation,&$blocked) { return $f->flock($operation,$blocked); }); temp_show($blocked);
        }
        temp_attempt(fn()=>$f->flock(LOCK_EX,3));
    }
    break;
case 'native':
    $f = temp_native(); $f->rewind(); temp_show($f->current()); temp_state($f);
    temp_show($f->fwrite('XY')); temp_show($f->current()); temp_state($f);
    temp_show($f->ftruncate(3)); temp_show($f->current()); temp_state($f);
    temp_show($f->fflush()); temp_show($f->fstat()['size']);
    $blocked = 'initial'; temp_show($f->flock(LOCK_EX|LOCK_NB,$blocked)); temp_show($blocked); temp_show($f->flock(LOCK_UN,$blocked));
    $f->rewind(); temp_show($f->fread(20));
    break;
case 'readonly':
    $f = temp_native('r'); temp_show($f->current()); temp_state($f);
    temp_attempt(fn()=>$f->fwrite('changed')); temp_state($f); temp_show($f->current());
    temp_attempt(fn()=>$f->ftruncate(2)); temp_state($f); temp_show($f->current());
    temp_show($f->fflush()); temp_show($f->fstat()['size']);
    break;
case 'factory':
    class TempPathProjection extends SplFileInfo { function __construct($path) { echo 'project:', $path, "\n"; parent::__construct($path); } }
    $f = new SplTempFileObject(); $f->setInfoClass(TempPathProjection::class);
    $parent = $f->getPathInfo(); temp_show([$parent::class,$parent->getPathname(),$parent->getPath(),$parent->getFilename()]);
    temp_attempt(fn()=>$f->getFileInfo()->getPathname());
    temp_attempt(fn()=>$f->getPathInfo(SplTempFileObject::class));
    temp_attempt(fn()=>$f->getPathInfo(stdClass::class));
    break;
case 'inheritance':
    class TempChild extends SplTempFileObject { public $note = 'owned'; function getCurrentLine(): string { echo "line override\n"; return 'tag:'.parent::fgets(); } }
    $f = new TempChild(2); $f->fwrite("first\nlast\n"); $f->rewind(); temp_show($f->current()); temp_show($f->__debugInfo());
    temp_attempt(fn()=>clone $f); temp_attempt(fn()=>serialize($f));
    class TempUninitialized extends SplTempFileObject { function __construct() {} }
    $f = new TempUninitialized();
    foreach (['fflush','fstat','__debugInfo'] as $method) temp_attempt(fn()=>$f->$method());
    temp_attempt(fn()=>$f->fwrite([])); temp_attempt(fn()=>$f->fwrite('x')); temp_attempt(fn()=>$f->ftruncate(-1));
    temp_attempt(fn()=>$f->flock(0));
    break;
case 'csv':
    foreach ([-1,0,8] as $limit) {
        $f = new SplTempFileObject($limit); $f->setCsvControl(';','"','');
        temp_show($f->fputcsv(['first','two;parts',"multi\nline"],';','"',''));
        $f->fwrite("last;end;tail\n"); $f->setFlags(SplFileObject::READ_CSV); $f->rewind();
        temp_show($f->current()); temp_state($f); $f->next(); temp_show($f->current()); temp_state($f);
        $f->seek(0); temp_show($f->fgetcsv()); temp_state($f);
    }
    break;
case 'lifetime':
    class TempLifetime extends SplTempFileObject { function __destruct() { echo 'retire:', $this->ftell(), ':', $this->fstat()['size'], "\n"; } }
    $f = new TempLifetime(1); $f->fwrite('kept'); $alias = $f; unset($f); echo "alias alive\n";
    $alias->rewind(); $copy = $alias->current(); $copy = 'local'; temp_show($alias->current()); unset($alias); echo "retired\n";
    break;
case 'buffer_boundaries':
    foreach ([-1,0,16384] as $limit) {
        foreach ([8190,8191,8192,8200] as $length) {
            $f = new SplTempFileObject($limit); $f->fwrite(str_repeat('q',$length)."\nZ\n"); $f->rewind();
            temp_show(strlen($f->fgets())); temp_state($f);
            temp_show($f->fgets()); temp_state($f); temp_show($f->valid());
            $f->setMaxLineLen(3); $f->rewind(); temp_show($f->fgets()); temp_state($f);
        }
    }
    break;
case 'wrapper_missing':
    class TempMissingWrapper {
        public $context;
        function url_stat($p,$f) { return ['mode'=>0100666]; }
        function stream_open($p,$m,$o,&$opened) { return true; }
    }
    stream_wrapper_register('tempmissing',TempMissingWrapper::class);
    $f = new SplFileObject('tempmissing://record','w+');
    temp_attempt(fn()=>$f->fstat()); temp_attempt(fn()=>$f->fflush());
    temp_attempt(fn()=>$f->ftruncate(2)); temp_attempt(fn()=>$f->flock(LOCK_EX)); temp_attempt(fn()=>$f->fwrite('x'));
    unset($f); stream_wrapper_unregister('tempmissing');
    class TempFalseFlush extends TempMissingWrapper {
        function stream_write($bytes) { echo "write\n"; return strlen($bytes); }
        function stream_flush() { echo "flush\n"; return false; }
        function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('tempfalseflush',TempFalseFlush::class);
    $f = new SplFileObject('tempfalseflush://record','w+'); $f->fwrite('x'); temp_show($f->fflush()); unset($f);
    stream_wrapper_unregister('tempfalseflush');
    break;
case 'wrapper':
    class TempMutationWrapper {
        public $context;
        public static $fail = '';
        function url_stat($p,$f) { return ['mode'=>0100666]; }
        function stream_open($p,$m,$o,&$opened) { return true; }
        function stream_write($data) { echo 'write:', bin2hex($data), "\n"; if(self::$fail==='write') throw new Exception('write stopped'); return min(2,strlen($data)); }
        function stream_flush() { echo "flush\n"; if(self::$fail==='flush') throw new Exception('flush stopped'); return true; }
        function stream_truncate($size) { echo 'truncate:', $size, "\n"; return $size<10; }
        function stream_lock($operation) { echo 'lock:', $operation, "\n"; return true; }
        function stream_stat() { echo "stat\n"; return ['size'=>7,'mode'=>0100666]; }
        function stream_close() { echo "close\n"; }
    }
    stream_wrapper_register('tempmutation', TempMutationWrapper::class);
    $f = new SplFileObject('tempmutation://record','w+');
    temp_show($f->fwrite('abcde')); temp_show($f->ftell()); temp_show($f->fflush());
    temp_show($f->ftruncate(3)); temp_show($f->ftruncate(12)); temp_show($f->fstat()['size']);
    $blocked = 'original'; temp_show($f->flock(LOCK_EX|LOCK_NB,$blocked)); temp_show($blocked);
    TempMutationWrapper::$fail = 'write'; temp_attempt(fn()=>$f->fwrite('stopped')); temp_show($f->ftell());
    TempMutationWrapper::$fail = 'flush'; temp_attempt(fn()=>$f->fflush());
    TempMutationWrapper::$fail = ''; unset($f); stream_wrapper_unregister('tempmutation');
    break;
}
} finally {
    foreach ($temp_paths as $path) if (file_exists($path)) unlink($path);
}
