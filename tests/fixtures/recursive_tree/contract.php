<?php
// Original recursive lookahead and tree projection contracts.
function tree_show($value) { echo json_encode($value, JSON_INVALID_UTF8_SUBSTITUTE), "\n"; }
function tree_attempt($fn) { try { tree_show($fn()); } catch (Throwable $e) { tree_show([$e::class,$e->getMessage()]); } }
set_error_handler(function($level,$message) { tree_show(['diagnostic',$level,$message]); return true; });
function tree_data() { return ['crown'=>['twig'=>'green','fork'=>['bud'=>'gold']],'ground'=>'brown']; }
class TreeInput implements RecursiveIterator {
    public static $trace = false;
    public $items;
    public $position = 0;
    public $failure = '';
    public function __construct($items) { $this->items=$items; }
    protected function event($name) { if(self::$trace) tree_show(['callback',$name,$this->position]); if($this->failure===$name) throw new Exception('failed '.$name); }
    public function rewind(): void { $this->event('rewind'); $this->position=0; }
    public function valid(): bool { $this->event('valid'); return $this->position<count($this->items); }
    public function current(): mixed { $this->event('current'); return array_values($this->items)[$this->position]??null; }
    public function key(): mixed { $this->event('key'); return array_keys($this->items)[$this->position]??null; }
    public function next(): void { $this->event('next'); $this->position++; }
    public function hasChildren(): bool { $this->event('hasChildren'); return is_array(array_values($this->items)[$this->position]??null); }
    #[ReturnTypeWillChange]
    public function getChildren() { $this->event('getChildren'); return new self(array_values($this->items)[$this->position]); }
}
function tree_cache_state($c) { return [$c->valid(),$c->key(),$c->current(),$c->getFlags(),$c->hasChildren(),$c->hasNext(),$c->getChildren()===null]; }
switch(getenv('RPHP_RECURSIVE_TREE_CASE')) {
case 'metadata':
    foreach(['RecursiveCachingIterator','RecursiveTreeIterator']as$class){
        $r=new ReflectionClass($class); tree_show([$class,$r->getParentClass()->getName(),$r->getInterfaceNames(),$r->getConstants()]);
        foreach($r->getMethods()as$m){
            if($m->getDeclaringClass()->getName()!==$class)continue;
            $params=[];foreach($m->getParameters()as$p)$params[]=[$p->getName(),(string)$p->getType(),$p->isPassedByReference(),$p->isOptional(),$p->isDefaultValueAvailable()?$p->getDefaultValue():'required',($p->isDefaultValueAvailable()&&$p->isDefaultValueConstant())?$p->getDefaultValueConstantName():null];
            tree_show([$m->getName(),$m->getNumberOfRequiredParameters(),(string)$m->getReturnType(),(string)$m->getTentativeReturnType(),$params]);
        }
    }
    break;
case 'cache_cursor':
    $inner=new RecursiveArrayIterator(tree_data());$c=new RecursiveCachingIterator($inner,0);
    tree_show(tree_cache_state($c));$c->rewind();tree_show(tree_cache_state($c));tree_show($inner->key());
    $c->next();tree_show(tree_cache_state($c));$c->next();tree_show(tree_cache_state($c));
    $c->rewind();tree_show(tree_cache_state($c));
    break;
case 'child_identity':
    $c=new RecursiveCachingIterator(new RecursiveArrayIterator(tree_data()),16);$c->rewind();$held=$c->getChildren();
    tree_show([$held::class,$held===$c->getChildren(),tree_cache_state($held)]);
    $held->rewind();tree_show(tree_cache_state($held));$c->next();tree_show([$c->getChildren(),$held->current()]);
    $c->rewind();tree_show($held===$c->getChildren());
    break;
case 'flags':
    foreach([0,1,2,4,8,16,17,256,272,65536,3,-1]as$flags){
        tree_attempt(function()use($flags){$c=new RecursiveCachingIterator(new RecursiveArrayIterator(tree_data()),$flags);$c->rewind();return [$flags,$c->getFlags(),$c->getChildren()?->getFlags()];});
    }
    $c=new RecursiveCachingIterator(new RecursiveArrayIterator(tree_data()),0);$c->rewind();$held=$c->getChildren();
    $c->setFlags(256);tree_show([$c->getFlags(),$held->getFlags()]);$c->rewind();tree_show($c->getChildren()->getFlags());
    break;
case 'tree_modes':
    foreach([0,4,8,12]as$flags)foreach([0,1,2]as$mode){
        $t=new RecursiveTreeIterator(new RecursiveArrayIterator(tree_data()),$flags,16,$mode);$rows=[];
        foreach($t as$k=>$v)$rows[]=[$k,$v,$t->getDepth(),$t->getPrefix(),$t->getEntry(),$t->getPostfix()];
        tree_show([$flags,$mode,$rows]);
    }
    break;
case 'tree_prefix':
    $t=new RecursiveTreeIterator(new RecursiveArrayIterator(tree_data()));
    for($i=0;$i<6;$i++)$t->setPrefixPart($i,'['.$i.']');$t->setPostfix('!');$t->setMaxDepth(1);
    tree_show(iterator_to_array($t,false));tree_show($t->getPostfix());
    tree_attempt(fn()=>$t->setPrefixPart(-1,'x'));tree_attempt(fn()=>$t->setPrefixPart(6,'x'));
    $t->setPrefixPart(5,"\xff\x00");$t->setPostfix("\x80");$t->rewind();tree_show([bin2hex($t->getPrefix()),bin2hex($t->current()),bin2hex($t->getPostfix())]);
    break;
case 'tree_empty':
    foreach([[],['single'=>'entry']]as$items)foreach([0,8,12]as$flags){
        $t=new RecursiveTreeIterator(new RecursiveArrayIterator($items),$flags);
        tree_attempt(fn()=>[$flags,$t->valid(),$t->key(),$t->current(),$t->getPrefix(),$t->getEntry(),$t->getDepth()]);
        tree_show(iterator_to_array($t));tree_attempt(fn()=>[$t->valid(),$t->key(),$t->current(),$t->getPrefix(),$t->getEntry(),$t->getDepth()]);
    }
    break;
case 'aggregate':
    class TreeAggregate implements IteratorAggregate { public function getIterator():Traversable { tree_show('getIterator');return new RecursiveArrayIterator(tree_data()); } }
    tree_show(iterator_to_array(new RecursiveTreeIterator(new TreeAggregate()),false));
    tree_attempt(fn()=>new RecursiveCachingIterator(new TreeAggregate()));
    class TreeFlatAggregate implements IteratorAggregate { public function getIterator():Traversable { tree_show('flat');return new ArrayIterator(['entry']); } }
    tree_attempt(fn()=>new RecursiveTreeIterator(new TreeFlatAggregate()));
    break;
case 'arguments':
    foreach([null,[],false,1,'x',new stdClass(),new ArrayIterator([])]as$input){tree_attempt(fn()=>new RecursiveCachingIterator($input));tree_attempt(fn()=>new RecursiveTreeIterator($input));}
    tree_attempt(fn()=>new RecursiveCachingIterator());tree_attempt(fn()=>new RecursiveTreeIterator());
    $i=new RecursiveArrayIterator([]);tree_attempt(fn()=>new RecursiveTreeIterator(iterator:$i,mode:1,cachingIteratorFlags:16,flags:8));
    tree_attempt(fn()=>new RecursiveTreeIterator($i,8,16,1,5));tree_attempt(fn()=>new RecursiveCachingIterator($i,0,1));
    break;
case 'strict':
    $f=eval('declare(strict_types=1);return function($i,$flags){return new RecursiveTreeIterator($i,$flags);};');
    foreach(['8',8.0,true,null,8]as$flags)tree_attempt(fn()=>$f(new RecursiveArrayIterator([]),$flags)->getPostfix());
    $t=new RecursiveTreeIterator(new RecursiveArrayIterator([]));
    $s=eval('declare(strict_types=1);return function($t,$p,$v){$t->setPrefixPart($p,$v);};');
    foreach([['1','x'],[1,2],[1,'x']]as$args)tree_attempt(fn()=>$s($t,...$args));
    break;
case 'uninitialized':
    class TreeMissing extends RecursiveTreeIterator { function __construct(){} }
    class CacheMissing extends RecursiveCachingIterator { function __construct(){} }
    $t=new TreeMissing();$c=new CacheMissing();
    foreach(['getPrefix','getEntry','getPostfix','current','key','rewind','valid']as$m)tree_attempt(fn()=>$t->$m());
    tree_attempt(fn()=>$t->setPostfix('x'));tree_attempt(fn()=>$t->setPrefixPart(0,'x'));
    foreach(['hasChildren','getChildren','current','getFlags','rewind']as$m)tree_attempt(fn()=>$c->$m());
    break;
case 'repeat':
    $c=new RecursiveCachingIterator(new RecursiveArrayIterator(tree_data()),0);$c->rewind();tree_attempt(fn()=>$c->__construct(new RecursiveArrayIterator([])));tree_show(tree_cache_state($c));
    $t=new RecursiveTreeIterator(new RecursiveArrayIterator(tree_data()));$t->setPostfix('old');$t->setPrefixPart(0,'changed');$t->setMaxDepth(0);$t->rewind();
    tree_attempt(fn()=>$t->__construct(new RecursiveArrayIterator(['new'=>['child'=>'leaf']])));tree_show([$t->getMaxDepth(),$t->getPostfix(),iterator_to_array($t,false)]);
    tree_attempt(fn()=>$t->__construct([]));$t->rewind();tree_show($t->current());
    break;
case 'callback_order':
    TreeInput::$trace=true;$source=new TreeInput(tree_data());$c=new RecursiveCachingIterator($source,0);
    tree_show('rewind');$c->rewind();tree_show('queries');$c->hasChildren();$c->getChildren();$c->getChildren();$c->hasNext();
    tree_show('next');$c->next();tree_show('stop');
    break;
case 'child_throw':
    foreach(['hasChildren','getChildren']as$method)foreach([0,16]as$flags){
        $i=new TreeInput(tree_data());$i->failure=$method;$c=new RecursiveCachingIterator($i,$flags);
        tree_attempt(fn()=>$c->rewind());tree_show([$method,$flags,$i->position,$c->valid(),$c->key(),$c->hasChildren(),$c->getChildren()]);
        $i->failure='';tree_attempt(fn()=>$c->next());tree_show([$i->position,$c->key(),$c->hasChildren()]);
    }
    break;
case 'invalid_child':
    class TreeBadChild extends TreeInput { #[ReturnTypeWillChange] public function getChildren(){return 17;} }
    foreach([0,16]as$flags){$i=new TreeBadChild(tree_data());$c=new RecursiveCachingIterator($i,$flags);tree_attempt(fn()=>$c->rewind());tree_show([$flags,$i->position,$c->valid(),$c->key(),$c->hasChildren(),$c->getChildren()]);}
    break;
case 'conversion_throw':
    class TreeStringValue { function __toString(){tree_show('convert');throw new Exception('text stopped');} }
    foreach([0,1,16,17]as$flags){$c=new RecursiveCachingIterator(new RecursiveArrayIterator([new TreeStringValue()]),$flags);tree_attempt(fn()=>$c->rewind());tree_show([$flags,$c->valid(),$c->hasChildren(),$c->hasNext(),$c->getFlags()]);}
    $t=new RecursiveTreeIterator(new RecursiveArrayIterator([new TreeStringValue()]));$t->rewind();tree_attempt(fn()=>$t->current());tree_show([$t->valid(),$t->key()]);
    break;
case 'overrides':
    class TreeDecorated extends RecursiveTreeIterator {
        public function getPrefix():string {tree_show('prefix override');return 'prefix';}
        public function getEntry():string {tree_show('entry override');return 'entry';}
        public function getPostfix():string {tree_show('postfix override');return 'postfix';}
    }
    $t=new TreeDecorated(new RecursiveArrayIterator(['key'=>'value']),0);$t->rewind();tree_show([$t->key(),$t->current(),$t->getPrefix(),$t->getEntry(),$t->getPostfix()]);
    break;
case 'full_cache':
    $c=new RecursiveCachingIterator(new RecursiveArrayIterator(tree_data()),256);$c->rewind();$child=$c->getChildren();tree_show([$c->getCache(),$child->getCache()]);
    $child->rewind();tree_show($child->getCache());$c->next();tree_show($c->getCache());
    $c['extra']='local';tree_show($c->getCache());$c->rewind();tree_show($c->getCache());
    break;
case 'reference_cow':
    $value='old';$items=['box'=>['ref'=>&$value],'next'=>'end'];$i=new RecursiveArrayIterator($items);$c=new RecursiveCachingIterator($i,0);$c->rewind();$held=$c->getChildren();$copy=$c->current();$copy['new']='only-copy';$value='changed';
    tree_show([$c->current(),$held->getInnerIterator()->getArrayCopy(),$items]);$held->rewind();tree_show($held->current());
    $i['box']=['replacement'];tree_show([$c->current(),$held->current()]);
    break;
case 'retirement':
    class TreeRetiredInput extends TreeInput {
        public $label;
        function __construct($items,$label='root'){parent::__construct($items);$this->label=$label;}
        #[ReturnTypeWillChange] public function getChildren(){return new self(array_values($this->items)[$this->position],'child');}
        function __destruct(){tree_show(['retire',$this->label]);}
    }
    $c=new RecursiveCachingIterator(new TreeRetiredInput(tree_data()),0);$c->rewind();$held=$c->getChildren();$c->next();tree_show('held');unset($c);tree_show('root gone');unset($held);tree_show('child gone');
    break;
case 'cycles':
    $a=['leaf'];$a['cycle']=&$a;$t=new RecursiveTreeIterator(new RecursiveArrayIterator($a),8);$t->setMaxDepth(4);$count=0;$deep=0;
    foreach($t as$k=>$v){$count++;$deep=max($deep,$t->getDepth());if($count>30)break;}
    tree_show([$count,$deep]);
    break;
case 'serialization':
    foreach([new RecursiveCachingIterator(new RecursiveArrayIterator([])),new RecursiveTreeIterator(new RecursiveArrayIterator([]))]as$o){
        tree_attempt(fn()=>clone$o);$encoded=serialize($o);tree_show($encoded);$copy=unserialize($encoded);tree_attempt(fn()=>$copy->rewind());
    }
    break;
case 'child_subclass':
    class TreeCacheChild extends RecursiveCachingIterator { public function __construct($i,$flags=0){tree_show(['child constructor',$flags]);parent::__construct($i,$flags);} }
    $c=new TreeCacheChild(new RecursiveArrayIterator(tree_data()),0);$c->rewind();$child=$c->getChildren();tree_show([$child::class,$child->getFlags()]);
    break;
case 'state_reentry':
    class TreeReentrant extends TreeInput { public $outer; public $once=true; #[ReturnTypeWillChange] public function getChildren(){if($this->once){$this->once=false;$this->outer->setFlags(256);tree_show(['during',$this->outer->key(),$this->outer->getChildren()]);}return parent::getChildren();} }
    $i=new TreeReentrant(tree_data());$c=new RecursiveCachingIterator($i,0);$i->outer=$c;$c->rewind();tree_show([$c->getFlags(),$c->getCache(),$c->getChildren()->getFlags()]);$i->outer=null;
    break;
case 'cache_exception_order':
    class TreeOrderValue { function __toString(){tree_show('text');throw new Exception('conversion failed');} }
    foreach(['CachingIterator','RecursiveCachingIterator']as$class)foreach([false,true]as$native){
        $items=[new TreeOrderValue()];$i=$native?new RecursiveArrayIterator($items):new TreeInput($items);$c=new $class($i,257);
        tree_attempt(fn()=>$c->rewind());tree_show([$class,$native,$c->getCache(),$i->valid(),$i->key()]);
    }
    break;
case 'retirement_reset':
    class TreeResetInput extends TreeInput {
        public $label;
        function __construct($items,$label='root'){parent::__construct($items);$this->label=$label;}
        #[ReturnTypeWillChange] public function getChildren(){tree_show('make child');return new self(array_values($this->items)[$this->position],'child');}
        function __destruct(){tree_show(['retire',$this->label]);}
    }
    TreeInput::$trace=true;$c=new RecursiveCachingIterator(new TreeResetInput(tree_data()),0);$c->rewind();tree_show('advance');$c->next();tree_show('release');unset($c);
    break;
case 'serialization_hooks':
    foreach(['IteratorIterator','CachingIterator','RecursiveCachingIterator','RecursiveIteratorIterator','RecursiveTreeIterator']as$class){
        $o=new $class(new RecursiveArrayIterator(['leaf']));
        tree_show([$class,serialize($o)]);
        tree_attempt(fn()=>clone $o);
        tree_show($o->__serialize());
    }
    class TreeWireOverride extends RecursiveTreeIterator {
        public function __serialize():array { tree_show('own hook');return ['kept'=>7]; }
        public function __unserialize(array $data):void { tree_show($data); }
    }
    $o=new TreeWireOverride(new RecursiveArrayIterator([]));
    $wire=serialize($o);tree_show($wire);unserialize($wire);
    break;
case 'interface_ancestry':
    class TreeUserCache extends CachingIterator {}
    class TreeUserRecursive extends RecursiveCachingIterator {}
    class TreeUserTree extends RecursiveTreeIterator {}
    foreach(['CachingIterator','RecursiveCachingIterator','TreeUserCache','TreeUserRecursive','TreeUserTree','DirectoryIterator','FilesystemIterator']as$class)tree_show([$class,(new ReflectionClass($class))->getInterfaceNames()]);
    break;
}
