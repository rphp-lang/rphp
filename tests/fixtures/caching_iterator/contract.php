<?php
// Original lookahead/cache contract, independently observed in reference PHP.
function cache_show($value) { echo json_encode($value, JSON_INVALID_UTF8_SUBSTITUTE), "\n"; }
function cache_attempt($action) {
    try { cache_show($action()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
function cache_state($it) { cache_show([$it->valid(), $it->key(), $it->current(), $it->hasNext(), $it->getFlags()]); }
set_error_handler(function($level, $message) { echo 'diagnostic:', $level, ':', $message, "\n"; return true; });
switch (getenv('RPHP_CACHING_ITERATOR_CASE')) {
case 'metadata':
    foreach (['__construct','rewind','valid','next','hasNext','__toString','getFlags','setFlags','offsetGet','offsetSet','offsetUnset','offsetExists','getCache','count'] as $name) {
        $m = new ReflectionMethod(CachingIterator::class, $name);
        echo $name, ':', $m->getNumberOfRequiredParameters(), '/', $m->getNumberOfParameters(), ':', $m->getReturnType(), ':', $m->getTentativeReturnType(), "\n";
        foreach ($m->getParameters() as $p) echo $p->getName(), ':', $p->getType(), ':', (int)$p->isOptional(), "\n";
    }
    cache_show([CachingIterator::CALL_TOSTRING,CachingIterator::TOSTRING_USE_KEY,CachingIterator::TOSTRING_USE_CURRENT,CachingIterator::TOSTRING_USE_INNER,CachingIterator::CATCH_GET_CHILD,CachingIterator::FULL_CACHE]);
    break;
case 'initial':
    $inner = new ArrayIterator(['first'=>'alpha','second'=>'beta']);
    $it = new CachingIterator($inner);
    cache_show([$it instanceof IteratorIterator,$it instanceof ArrayAccess,$it instanceof Countable,$it instanceof Stringable,$it->getInnerIterator() === $inner]);
    cache_state($it); cache_show((string)$it);
    $it->next(); cache_state($it); cache_show([$inner->key(),$inner->current(),(string)$it]);
    $it->rewind(); cache_state($it); cache_show([$inner->key(),$inner->current(),(string)$it]);
    $it->next(); cache_state($it); cache_show((string)$it);
    $it->next(); cache_state($it); cache_show((string)$it);
    break;
case 'empty':
    foreach ([0,1,2,4,256] as $flags) {
        $it = new CachingIterator(new ArrayIterator([]), $flags);
        cache_state($it); cache_attempt(fn() => (string)$it);
        $it->rewind(); cache_state($it); $it->next(); cache_state($it);
    }
    break;
case 'lookahead':
    $inner = new ArrayIterator(['a'=>null,'b'=>false,'c'=>'last']);
    $it = new CachingIterator($inner, 0);
    for($it->rewind();$it->valid();$it->next()) {
        cache_state($it); cache_show([$inner->valid(),$inner->key(),$inner->current()]);
        cache_show([$it->hasNext(),$it->hasNext()]);
    }
    cache_state($it);
    break;
case 'cache':
    $it = new CachingIterator(new ArrayIterator(['a'=>'alpha','b'=>null,'c'=>'gamma']), CachingIterator::FULL_CACHE);
    cache_show([$it->getCache(),count($it)]);
    $it['seed'] = 4; cache_show($it->getCache());
    $it->rewind(); cache_state($it); cache_show([$it->getCache(),count($it)]);
    $snapshot = $it->getCache(); $snapshot['a'] = 'detached';
    $it->next(); cache_show([$it->getCache(),$snapshot,count($it),isset($it['b']),$it->offsetExists('b'),$it['b']]);
    $it->next(); $it->next(); cache_show([$it->getCache(),count($it)]);
    $it->rewind(); cache_show([$it->getCache(),count($it)]);
    break;
case 'offsets':
    $it = new CachingIterator(new ArrayIterator([]), CachingIterator::FULL_CACHE);
    foreach ([null,false,true,3,2.5,'03','3','',[],new stdClass()] as $key) {
        cache_attempt(function() use ($it,$key) { $it->offsetSet($key,'stored'); return $it->getCache(); });
        cache_attempt(fn() => $it->offsetExists($key));
        cache_attempt(fn() => $it->offsetGet($key));
        cache_attempt(function() use ($it,$key) { $it->offsetUnset($key); return $it->getCache(); });
    }
    cache_attempt(fn() => $it['absent']); $it[] = 6; cache_show($it->getCache());
    break;
case 'flags':
    $inner = new ArrayIterator(['one']);
    foreach ([0,1,2,4,8,16,256,257,258,260,264,3,5,6,7,9,15,-1,512,null,false,'2',2.5,[]] as $flags) {
        cache_attempt(fn() => (new CachingIterator($inner,$flags))->getFlags());
    }
    break;
case 'setflags':
    foreach ([0,1,2,4,8,256,257] as $start) {
        echo 'start:', $start, "\n";
        $it = new CachingIterator(new ArrayIterator(['a'=>'alpha','b'=>'beta']),$start);
        foreach ([0,1,2,4,8,3,256,257,0] as $flags) {
            cache_attempt(fn() => $it->setFlags($flags)); cache_show($it->getFlags());
        }
    }
    $it = new CachingIterator(new ArrayIterator(['a'=>1,'b'=>2]),256);
    $it->rewind(); $it['extra'] = 9; $it->setFlags(0);
    cache_attempt(fn() => $it->getCache()); $it->setFlags(256); cache_show($it->getCache());
    break;
case 'strings':
    class CacheStringInput extends ArrayIterator { function __toString(): string { echo "inner cast\n"; return 'inner-'.$this->key(); } }
    class CacheStringValue { function __toString(): string { echo "value cast\n"; return 'value'; } }
    foreach ([0,1,2,4,8] as $flags) {
        echo 'flags:', $flags, "\n";
        $it = new CachingIterator(new CacheStringInput(['a'=>new CacheStringValue(),'b'=>23]),$flags);
        cache_attempt(fn() => (string)$it);
        cache_attempt(fn() => $it->rewind());
        cache_attempt(fn() => (string)$it); cache_attempt(fn() => (string)$it);
        cache_attempt(fn() => $it->next()); cache_attempt(fn() => (string)$it);
        cache_attempt(fn() => $it->next()); cache_attempt(fn() => (string)$it);
    }
    break;
case 'order':
case 'throw':
    class OriginalCacheInput implements Iterator {
        public $position = 0;
        public static $failure = '';
        function event($name) { echo $name, ':', $this->position, "\n"; if(self::$failure === $name) throw new Exception('input stopped'); }
        function rewind(): void { $this->event('rewind'); $this->position = 0; }
        function valid(): bool { $this->event('valid'); return $this->position < 2; }
        function current(): mixed { $this->event('current'); return ['alpha','beta'][$this->position]; }
        function key(): mixed { $this->event('key'); return $this->position + 7; }
        function next(): void { $this->event('next'); ++$this->position; }
        function __destruct() { echo "input retired\n"; }
    }
    if(getenv('RPHP_CACHING_ITERATOR_CASE') === 'order') {
        $it = new CachingIterator(new OriginalCacheInput(),256);
        cache_state($it); cache_show(iterator_to_array($it)); cache_show($it->getCache()); unset($it);
    } else {
        foreach(['rewind','valid','current','key','next'] as $failure) {
            echo 'fail:', $failure, "\n";
            $it = new CachingIterator(new OriginalCacheInput(),256);
            OriginalCacheInput::$failure = $failure;
            cache_attempt(fn() => $it->rewind());
            OriginalCacheInput::$failure = '';
            cache_state($it); cache_show($it->getCache()); unset($it);
        }
    }
    break;
case 'conversion':
    class OriginalCacheText {
        public $owner;
        public $action;
        function __toString(): string {
            echo "convert\n";
            if($this->action === 'throw') throw new Exception('conversion stopped');
            $this->owner->setFlags(CachingIterator::CALL_TOSTRING | CachingIterator::FULL_CACHE);
            return 'text';
        }
    }
    foreach(['change','throw'] as $action) {
        $text = new OriginalCacheText(); $text->action = $action;
        $it = new CachingIterator(new ArrayIterator([$text,'tail'])); $text->owner = $it;
        cache_attempt(fn() => $it->rewind());
        cache_show([$it->valid(),$it->key(),$it->hasNext(),$it->getFlags(),gettype($it->current())]);
        cache_attempt(fn() => (string)$it);
        unset($text->owner,$it,$text); gc_collect_cycles();
    }
    break;
case 'reference':
    $text = 'before'; $inner = new ArrayIterator(['x'=>&$text,'y'=>['n'=>2]]);
    $it = new CachingIterator($inner,257); $it->rewind(); $text = 'after';
    cache_show([$it->current(),(string)$it,$it->getCache()]);
    $copy = $it->current(); $copy = 'local'; cache_show([$it->current(),$text]);
    cache_attempt(fn() => $it->setFlags(256)); cache_attempt(fn() => $it->next()); cache_show([$it->current(),$it->getCache()]);
    break;
case 'cow':
    $it = new CachingIterator(new ArrayIterator(['key'=>['v'=>1]]),256); $it->rewind();
    $cache = $it->getCache(); $cache['key']['v'] = 2;
    $current = $it->current(); $current['v'] = 3;
    $slot = $it['key']; $slot['v'] = 4;
    cache_show([$it->getCache(),$it->current(),$cache,$current,$slot]);
    $it['key']['v'] = 5; cache_show($it->getCache());
    break;
case 'live':
    $inner = new ArrayIterator(['a'=>1,'b'=>2,'c'=>3]); $it = new CachingIterator($inner,256);
    $it->rewind(); $inner['b'] = 22; unset($inner['a']); $inner['d'] = 4;
    cache_state($it); $it->next(); cache_state($it); cache_show($it->getCache());
    $inner->rewind(); $it->next(); cache_state($it); cache_show($it->getCache());
    break;
case 'strict':
    $call = eval('declare(strict_types=1); return function($i,$f) { $i->setFlags($f); };');
    $it = new CachingIterator(new ArrayIterator([]),0);
    foreach([null,false,1.0,'2',2] as $flags) { cache_attempt(fn() => $call($it,$flags)); cache_show($it->getFlags()); }
    break;
case 'uninitialized':
    class OriginalEmptyCache extends CachingIterator { function __construct() {} }
    $it = new OriginalEmptyCache();
    foreach(['rewind','valid','next','hasNext','__toString','getFlags','getCache','count','key','current','getInnerIterator'] as $name) cache_attempt(fn() => $it->$name());
    cache_attempt(fn() => $it->setFlags(0)); cache_attempt(fn() => $it->offsetGet('x'));
    cache_attempt(fn() => $it->offsetSet('x',1)); cache_attempt(fn() => $it->offsetUnset('x'));
    break;
case 'nocache':
    $it = new CachingIterator(new ArrayIterator([1,2]),0);
    cache_attempt(fn() => $it->getCache()); cache_attempt(fn() => count($it));
    cache_attempt(fn() => $it->offsetGet('x')); cache_attempt(fn() => $it->offsetExists('x'));
    cache_attempt(fn() => $it->offsetSet('x',1)); cache_attempt(fn() => $it->offsetUnset('x'));
    break;
case 'lifetime':
    class OriginalCacheLifetime extends ArrayIterator { function __destruct() { echo "retired\n"; } }
    $inner = new OriginalCacheLifetime(['a','b']); $it = new CachingIterator($inner,256); $alias = $it;
    unset($inner,$it); cache_show(iterator_to_array($alias));
    cache_attempt(fn() => clone $alias); cache_attempt(fn() => $alias->__construct(new ArrayIterator([])));
    unset($alias); gc_collect_cycles();
    break;
case 'bytes':
    $it = new CachingIterator(new ArrayIterator(["\xff"=>"a\x00\xff","\xc3\xa9"=>"\xc3\xa9"]),257);
    foreach($it as $key=>$value) cache_show([bin2hex($key),bin2hex($value),bin2hex((string)$it)]);
    foreach($it->getCache() as $key=>$value) cache_show([bin2hex($key),bin2hex($value)]);
    break;
case 'diagnostics':
    class OriginalNamedCache extends CachingIterator {}
    $it = new OriginalNamedCache(new ArrayIterator([]),0);
    cache_attempt(fn() => $it->getCache()); cache_attempt(fn() => (string)$it);
    cache_attempt(fn() => $it->__construct(new ArrayIterator([])));
    $it->setFlags(256);
    foreach([0,3,'03','absent'] as $key) cache_attempt(fn() => $it->offsetGet($key));
    break;
case 'cycle':
    class OriginalCycledCache extends CachingIterator { function __destruct() { echo "cache retired\n"; } }
    class OriginalCachedOwner { function __destruct() { echo "value retired\n"; } }
    $value = new OriginalCachedOwner();
    $it = new OriginalCycledCache(new ArrayIterator([$value]),256);
    $value->owner = $it; $it->rewind();
    unset($value,$it); gc_collect_cycles(); echo "after collection\n";
    break;
default: throw new Exception('unknown original caching iterator case');
}
restore_error_handler();
