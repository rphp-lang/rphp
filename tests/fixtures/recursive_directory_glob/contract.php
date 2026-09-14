<?php
// Original filesystem cursor projections; each case receives a fresh directory.
function cursor_show($value) { echo json_encode($value, JSON_INVALID_UTF8_SUBSTITUTE), "\n"; }
function cursor_try($operation) { try { cursor_show($operation()); } catch (Throwable $e) { cursor_show([$e::class,$e->getMessage()]); } }
set_error_handler(function($level,$message) { cursor_show(['diagnostic',$level,$message]); return true; });
function cursor_find($iterator,$name) {
    $iterator->rewind();
    while($iterator->valid() && $iterator->getFilename()!==$name) $iterator->next();
    if(!$iterator->valid()) throw new Exception('missing fixture entry');
}
function cursor_names($iterator) { $out=[];foreach($iterator as $entry)$out[]=$iterator->getFilename();sort($out);return $out; }
function cursor_state($iterator) { return [$iterator->valid(),$iterator->getFlags(),$iterator->getPath(),$iterator->getPathname(),$iterator->getFilename(),$iterator->key()]; }
function recursive_state($iterator) { return [cursor_state($iterator),$iterator->getSubPath(),$iterator->getSubPathname(),$iterator->hasChildren()]; }
switch(getenv('RPHP_RECURSIVE_DIRECTORY_GLOB_CASE')) {
case 'metadata':
    foreach(['RecursiveDirectoryIterator','GlobIterator'] as $class) {
        $r=new ReflectionClass($class);cursor_show([$class,$r->getParentClass()->getName(),$r->getInterfaceNames(),$r->getConstants()]);
        foreach($r->getMethods() as $m) {
            if($m->getDeclaringClass()->getName()!==$class)continue;
            $params=[];
            foreach($m->getParameters() as $p)$params[]=[$p->getName(),(string)$p->getType(),$p->isPassedByReference(),$p->isOptional(),$p->isDefaultValueAvailable()?$p->getDefaultValue():'required',($p->isDefaultValueAvailable()&&$p->isDefaultValueConstant())?$p->getDefaultValueConstantName():null,(string)$p];
            cursor_show([$m->getName(),$m->getNumberOfRequiredParameters(),(string)$m->getReturnType(),(string)$m->getTentativeReturnType(),$params]);
        }
    }
    break;
case 'recursive_flags':
    foreach([0,4096,0x1120,0x4010,-1] as $flags) {
        $i=new RecursiveDirectoryIterator('grove',$flags);cursor_show([$flags,$i->getFlags(),cursor_names($i)]);
        cursor_find($i,'stem');cursor_show(recursive_state($i));cursor_show($i->current()===$i);
        $child=$i->getChildren();cursor_show([$child::class,$child->getFlags(),$child->getSubPath(),cursor_names($child)]);
    }
    break;
case 'recursive_dots':
    $i=new RecursiveDirectoryIterator('grove');
    foreach(['.','..','leaf.txt','stem','empty'] as $name) { cursor_find($i,$name);cursor_show([$name,$i->isDot(),$i->hasChildren(),$i->hasChildren(true)]); }
    break;
case 'recursive_subpaths':
    $root=new RecursiveDirectoryIterator('grove///',4096);cursor_find($root,'stem');
    $child=$root->getChildren();cursor_find($child,'branch');$grand=$child->getChildren();cursor_find($grand,'seed.txt');
    cursor_show(recursive_state($root));cursor_show(recursive_state($child));cursor_show(recursive_state($grand));
    $root->rewind();cursor_show([$grand->getSubPath(),$grand->getSubPathname()]);
    break;
case 'recursive_traversal':
    foreach([0,1,2] as $mode) {
        $i=new RecursiveIteratorIterator(new RecursiveDirectoryIterator('grove',4096),$mode);$rows=[];
        foreach($i as $key=>$entry)$rows[]=[$key,$entry->getFilename(),$i->getDepth(),$i->getSubIterator()->getSubPathname()];
        sort($rows);cursor_show([$mode,$rows]);
    }
    break;
case 'child_identity':
    $root=new RecursiveDirectoryIterator('grove',4096);cursor_find($root,'stem');
    $a=$root->getChildren();$b=$root->getChildren();cursor_find($a,'branch');cursor_find($b,'bud.txt');
    cursor_show([$a===$b,$a->getFilename(),$b->getFilename()]);$root->setFlags(0x1120);$c=$root->getChildren();
    cursor_show([$a->getFlags(),$c->getFlags(),$a->getSubPath(),$c->getSubPath()]);
    unset($root,$b,$c);cursor_show($a->getChildren()->getSubPath());
    break;
case 'child_subclass':
    class SproutCursor extends RecursiveDirectoryIterator {
        public array $payload=['base'];
        public function __construct($path,$flags=4096) { cursor_show(['construct',$path,$flags]);parent::__construct($path,$flags); }
    }
    $root=new SproutCursor('grove');$root->payload[]='root';cursor_find($root,'stem');$child=$root->getChildren();
    cursor_show([$child::class,$child->payload,$child->getSubPath()]);
    $root->setInfoClass(SplFileObject::class);$child=$root->getChildren();cursor_find($child,'bud.txt');
    cursor_show([$child::class,$child->current()::class]);
    break;
case 'child_constructor_throw':
    class ThrowingSprout extends RecursiveDirectoryIterator {
        public static bool $fail=false;
        public function __construct($path,$flags=4096) { cursor_show(['construct',$path,$flags]);if(self::$fail)throw new Exception('child rejected');parent::__construct($path,$flags); }
    }
    $root=new ThrowingSprout('grove');cursor_find($root,'stem');ThrowingSprout::$fail=true;
    cursor_try(fn()=>$root->getChildren());cursor_show(recursive_state($root));ThrowingSprout::$fail=false;
    cursor_show($root->getChildren()->getSubPath());
    break;
case 'symlinks':
    foreach([4096,0x5000] as $flags){
        $i=new RecursiveDirectoryIterator('links',$flags);
        foreach(['to-dir','to-file','dangling'] as $name){cursor_find($i,$name);cursor_show([$flags,$name,$i->isLink(),$i->hasChildren(),$i->hasChildren(true),$i->hasChildren(false)]);}
        cursor_find($i,'to-dir');cursor_try(fn()=>$i->getChildren()->getSubPath());
    }
    break;
case 'recursive_child_errors':
    $i=new RecursiveDirectoryIterator('grove',4096);cursor_find($i,'leaf.txt');cursor_try(fn()=>$i->getChildren());cursor_show(recursive_state($i));
    cursor_find($i,'empty');rmdir('grove/empty');cursor_try(fn()=>$i->getChildren());cursor_show($i->getFilename());
    while($i->valid())$i->next();cursor_show([$i->getSubPath(),$i->getSubPathname(),$i->hasChildren()]);cursor_try(fn()=>$i->getChildren());
    break;
case 'recursive_clone':
    $root=new RecursiveDirectoryIterator('grove',4096);cursor_find($root,'stem');$child=$root->getChildren();cursor_find($child,'bud.txt');$copy=clone $child;
    cursor_show([$copy===$child,recursive_state($copy)]);$child->next();cursor_show($copy->getFilename());$copy->rewind();cursor_show($child->getFilename());
    unset($root,$child);cursor_show(cursor_names($copy));
    break;
case 'arguments':
    foreach(['RecursiveDirectoryIterator','GlobIterator'] as $class){
        foreach(['',"grove\0suffix",null,[],false,123] as $arg)cursor_try(fn()=>(new $class($arg))->getFlags());
        cursor_try(fn()=>new $class());cursor_try(fn()=>new $class('grove',0,1));
        foreach([null,'4096',4096.5,[],true] as $flags)cursor_try(fn()=>(new $class('grove',$flags))->getFlags());
    }
    cursor_show((new RecursiveDirectoryIterator(flags:4096,directory:'grove'))->getFlags());
    cursor_show((new GlobIterator(flags:0,pattern:'grove/*.txt'))->count());
    break;
case 'strict':
    $make=eval('declare(strict_types=1);return function($c,$p,$f){return new $c($p,$f);};');
    foreach(['RecursiveDirectoryIterator','GlobIterator'] as $class)foreach([[123,0],['grove','0'],['grove',false],['grove',0]] as [$p,$f])cursor_try(fn()=>$make($class,$p,$f)->getFlags());
    $has=eval('declare(strict_types=1);return function($i,$a){return $i->hasChildren($a);};');
    $i=new RecursiveDirectoryIterator('grove');cursor_find($i,'stem');foreach([0,null,'1',true] as $arg)cursor_try(fn()=>$has($i,$arg));
    break;
case 'uninitialized':
    class BareRecursive extends RecursiveDirectoryIterator { public function __construct() {} }
    class BareGlob extends GlobIterator { public function __construct() {} }
    foreach([new BareRecursive(),new BareGlob()] as $i){
        foreach(['getFlags','getPath','getPathname','getFilename','current','key','rewind','valid','next'] as $method)cursor_try(fn()=>$i->$method());
        if($i instanceof RecursiveDirectoryIterator)foreach(['getSubPath','getSubPathname','hasChildren','getChildren'] as $method)cursor_try(fn()=>$i->$method());
        else {cursor_try(fn()=>$i->count());cursor_try(fn()=>count($i));}
    }
    $i=(new ReflectionClass(GlobIterator::class))->newInstanceWithoutConstructor();cursor_try(fn()=>count($i));
    break;
case 'repeat':
    foreach([new RecursiveDirectoryIterator('grove',4096),new GlobIterator('grove/*.txt')] as $i){
        cursor_try(fn()=>$i->__construct('absent'));cursor_try(fn()=>$i->__construct([]));cursor_show([$i->valid(),$i->getFlags(),$i->getPath()]);
    }
    class RecoverRecursive extends RecursiveDirectoryIterator { function __construct(){} function initialize($path){parent::__construct($path);} }
    $i=new RecoverRecursive();cursor_try(fn()=>$i->initialize('absent'));cursor_try(fn()=>$i->getPathname());cursor_try(fn()=>$i->initialize('grove'));
    break;
case 'glob_patterns':
    foreach(['grove/*.txt','grove/*/*.txt','*.note','./*.note','grove//*.txt','missing*','grove/{leaf,bud}.txt','grove/.*','glob://grove/*.txt'] as $pattern){
        $i=new GlobIterator($pattern);$rows=[];foreach($i as $key=>$info)$rows[]=[$key,$info->getPathname(),$i->getPath(),$i->getFilename(),(string)$i];
        cursor_show([$pattern,$i->count(),$i->getFlags(),$rows,cursor_state($i)]);
    }
    break;
case 'glob_flags':
    foreach([0,0x20,0x110,4096,-1] as $flags){$i=new GlobIterator('grove/*',$flags);$rows=[];foreach($i as $k=>$v)$rows[]=[$k,is_string($v)?$v:$v->getPathname(),$v===$i];cursor_show([$flags,$i->getFlags(),count($i),$rows]);}
    break;
case 'glob_snapshot':
    $i=new GlobIterator('*.note');cursor_show([count($i),cursor_names($i)]);file_put_contents('new.note','new');unlink('first.note');
    cursor_show([count($i),cursor_names($i),cursor_names(new GlobIterator('*.note'))]);$i->rewind();$held=$i->current();$i->next();
    cursor_show([$held->getPathname(),$i->getPathname()]);$i->seek(0);cursor_show(cursor_state($i));cursor_try(fn()=>$i->seek(99));cursor_show(count($i));
    break;
case 'glob_dot_flags':
    foreach([0,4096] as $flags){
        $g=new GlobIterator('grove/.*',$flags);cursor_show([count($g),cursor_names($g)]);
        $g->rewind();cursor_show(cursor_state($g));$g->seek(0);cursor_show(cursor_state($g));
        cursor_try(fn()=>$g->seek(1));cursor_show(cursor_state($g));
    }
    $g=new GlobIterator('grove/.*');$g->setFlags(4096);$g->rewind();cursor_show([count($g),cursor_names($g)]);
    break;
case 'glob_first_class':
    class CaptureGlob extends GlobIterator {
        function __construct($ready){if($ready)parent::__construct('*.note');}
        function visit($arg){cursor_show(['captured body',$arg]);return 31;}
    }
    $live=new CaptureGlob(true);$bare=new CaptureGlob(false);
    cursor_try(function()use($bare){$f=$bare->visit(...);return $f('bare');});
    foreach([$live,$bare,$live,$bare] as $g)cursor_try(function()use($g){$f=$g->visit(...);return $f('cached');});
    $method='visit';cursor_try(function()use($bare,$method){$f=$bare->$method(...);return $f('dynamic');});
    $cb=[$bare,'visit'];cursor_try(function()use($cb){$f=$cb(...);return $f('array');});
    foreach([[$bare,'visit',0],[$bare],[$bare,2],[42,'visit'],[new stdClass,'missing',0]] as $cb)
        cursor_try(function()use($cb){return $cb(...);});
    class MagicCaptureGlob extends GlobIterator {
        function __construct($ready){if($ready)parent::__construct('*.note');}
        function __call($name,$args){cursor_show(['magic capture',$name,$args]);return 53;}
    }
    $live=new MagicCaptureGlob(true);$bare=new MagicCaptureGlob(false);
    foreach([$live,$bare,$live,$bare] as $g)cursor_try(function()use($g){$f=$g->missing(...);return $f('magic');});
    break;
case 'glob_clone':
    $i=new GlobIterator('*.note');$i->seek(1);cursor_try(fn()=>clone $i);cursor_show([cursor_state($i),count($i)]);
    class PatternCursor extends GlobIterator { public function __clone(){cursor_show('clone callback');} }
    $child=new PatternCursor('*.note');cursor_try(fn()=>clone $child);cursor_show(cursor_names($child));
    break;
case 'glob_empty':
    $i=new GlobIterator('no-such-entry-*');foreach(['count','valid','getPath','getPathname','getFilename','key','current','rewind','next'] as $m)cursor_try(fn()=>$i->$m());
    cursor_try(fn()=>$i->seek(1));cursor_show(count($i));
    break;
case 'raw_bytes':
    $i=new RecursiveDirectoryIterator("raw",4096);cursor_find($i,"dir-\xff");$child=$i->getChildren();cursor_find($child,"leaf-\x80.dat");
    cursor_show([bin2hex($child->getPathname()),bin2hex($child->getSubPath()),bin2hex($child->getSubPathname())]);
    foreach(["raw/*/*", "raw/dir-\xff/*"] as $p){$i=new GlobIterator($p);$rows=[];foreach($i as $k=>$v)$rows[]=[bin2hex($k),bin2hex($v->getPathname()),bin2hex($i->getFilename())];cursor_show($rows);}
    break;
case 'glob_empty_metadata':
    $g=new GlobIterator('nothing-*.absent');
    foreach(['getSize','getATime','getCTime','getMTime','getPerms','getInode','getOwner','getGroup','getType','isDir','isFile','isLink','isReadable','isWritable','isExecutable'] as $m)cursor_try(fn()=>[$m,$g->$m()]);
    cursor_show($g->getRealPath()===realpath('.'));cursor_show($g->__debugInfo());
    $g=new GlobIterator('grove/*.txt');cursor_show($g->__debugInfo());$g->next();cursor_show($g->__debugInfo());
    break;
case 'glob_method_lookup':
    class GuardedGlob extends GlobIterator {
        function __construct($ready){if($ready)parent::__construct('*.note');}
        function visit($arg){cursor_show(['body',$arg]);return 19;}
        function count():int { cursor_show('count override');return 71; }
    }
    $live=new GuardedGlob(true);$bare=new GuardedGlob(false);
    foreach([$live,$bare,$live,$bare] as $g)cursor_try(fn()=>$g->visit(cursor_show('argument')));
    cursor_try(fn()=>$bare->count());cursor_show(count($bare));cursor_show(call_user_func([$bare,'visit'],'callback'));
    break;
case 'reference_cow':
    $i=new RecursiveDirectoryIterator('grove',4096);cursor_find($i,'stem');$child=$i->getChildren();cursor_find($child,'bud.txt');
    $path=$child->getPathname();$alias=&$path;$copy=$path;$alias[0]='X';$info=$child->current();$child->next();
    cursor_show([$copy,$path,$info->getPathname(),$child->getSubPath()]);
    $g=new GlobIterator('*.note');$name=$g->key();$held=$g->current();$g->next();cursor_show([$name,$held->getPathname(),$g->key()]);
    break;
case 'retirement':
    class RetiringDirectory extends RecursiveDirectoryIterator { public function __destruct(){cursor_show(['drop',$this->getSubPath()]);} }
    $root=new RetiringDirectory('grove',4096);cursor_find($root,'stem');$child=$root->getChildren();unset($root);cursor_find($child,'branch');$grand=$child->getChildren();unset($child);
    cursor_show(cursor_names($grand));unset($grand);
    $g=new GlobIterator('*.note');$held=$g->current();unset($g);cursor_show($held->getFilename());
    break;
}
