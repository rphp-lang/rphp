<?php
// Original public-list and selected-cursor contracts, observed independently.
function append_show($value) { echo json_encode($value, JSON_INVALID_UTF8_SUBSTITUTE), "\n"; }
function append_attempt($action) {
    try { append_show($action()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
function append_state($it) {
    $list = $it->getArrayIterator();
    append_show([$it->valid(),$it->key(),$it->current(),$it->getIteratorIndex(),$it->getInnerIterator() !== null,$list->key(),$list->valid()]);
}
set_error_handler(function($level, $message) { echo 'diagnostic:', $level, ':', $message, "\n"; return true; });
switch (getenv('RPHP_APPEND_ITERATOR_CASE')) {
case 'metadata':
    foreach (['__construct','append','rewind','valid','current','key','next','getInnerIterator','getIteratorIndex','getArrayIterator'] as $name) {
        $m = new ReflectionMethod(AppendIterator::class, $name);
        echo $name, ':', $m->getNumberOfRequiredParameters(), '/', $m->getNumberOfParameters(), ':', $m->getReturnType(), ':', $m->getTentativeReturnType(), "\n";
        foreach ($m->getParameters() as $p) echo $p->getName(), ':', $p->getType(), ':', (int)$p->isOptional(), "\n";
    }
    $it = new AppendIterator();
    append_show([$it instanceof IteratorIterator, $it instanceof OuterIterator, $it->getArrayIterator() instanceof ArrayIterator]);
    break;
case 'empty':
    $it = new AppendIterator(); append_state($it);
    append_attempt(fn() => $it->rewind()); append_state($it);
    append_attempt(fn() => $it->next()); append_state($it);
    append_show($it->getArrayIterator() === $it->getArrayIterator());
    append_attempt(fn() => $it->append(new ArrayIterator([]))); append_state($it);
    append_attempt(fn() => $it->__construct());
    append_attempt(fn() => clone $it);
    break;
case 'append':
    $it = new AppendIterator();
    foreach ([[],['left'=>null,'right'=>false],[],['tail'=>'done']] as $values) {
        append_attempt(fn() => $it->append(new ArrayIterator($values))); append_state($it);
    }
    for ($i=0;$i<5;++$i) { $it->next(); append_state($it); }
    $it->rewind(); append_state($it);
    append_show(iterator_to_array($it));
    break;
case 'exhausted':
    $it = new AppendIterator(); $it->append(new ArrayIterator(['a'=>1]));
    $it->next(); append_state($it);
    $it->append(new ArrayIterator(['b'=>2,'c'=>3])); append_state($it);
    $it->next(); append_state($it); $it->next(); append_state($it);
    $it->getArrayIterator()->append(new ArrayIterator(['d'=>4])); append_state($it);
    $it->next(); append_state($it); $it->rewind(); append_state($it);
    break;
case 'duplicate':
    $inner = new ArrayIterator(['x'=>1,'y'=>2]); $it = new AppendIterator();
    $it->append($inner); $it->append($inner);
    for($it->rewind(),$i=0;$it->valid() && $i<7;$it->next(),++$i) append_state($it);
    append_state($it); append_show($it->getArrayIterator()->count());
    break;
case 'inner_mutation':
    $inner = new ArrayIterator(['first'=>10,'second'=>20,'third'=>30]);
    $it = new AppendIterator(); $it->append($inner); append_state($it);
    $inner->next(); append_state($it);
    $inner['first']=11; $inner['second']=22; append_state($it);
    $it->next(); append_state($it);
    $inner->rewind(); append_state($it); $it->next(); append_state($it);
    break;
case 'list_mutation':
    $it = new AppendIterator(); $first = new ArrayIterator(['a'=>1,'b'=>2]);
    $second = new ArrayIterator(['c'=>3]); $third = new ArrayIterator(['d'=>4]);
    $it->append($first); $it->append($second); $list=$it->getArrayIterator();
    append_state($it); $list->next(); append_state($it);
    $it->next(); append_state($it);
    $list[0]=$third; append_state($it); $it->rewind(); append_state($it);
    unset($list[0]); append_state($it); $it->next(); append_state($it);
    $list->append($first); $it->rewind(); append_state($it);
    append_show(iterator_to_array($it));
    break;
case 'list_key':
    $it = new AppendIterator(); $list=$it->getArrayIterator();
    $list['custom']=new ArrayIterator(['x'=>'custom']); $list[8]=new ArrayIterator(['y'=>'numeric']);
    $it->rewind(); append_state($it); $it->next(); append_state($it); $it->next(); append_state($it);
    break;
case 'order':
case 'throw':
    class AppendObservedInput implements Iterator {
        public $position=0;
        public static $failure='';
        function event($name) { echo $name, ':', $this->position, "\n"; if(self::$failure===$name) throw new Exception('cursor stopped'); }
        function rewind(): void { $this->event('rewind'); $this->position=0; }
        function valid(): bool { $this->event('valid'); return $this->position<2; }
        function current(): mixed { $this->event('current'); return ['first','last'][$this->position]; }
        function key(): mixed { $this->event('key'); return $this->position+9; }
        function next(): void { $this->event('next'); ++$this->position; }
        function __destruct() { echo "input retired\n"; }
    }
    if(getenv('RPHP_APPEND_ITERATOR_CASE')==='order') {
        $it=new AppendIterator(); $it->append(new AppendObservedInput()); append_state($it);
        $it->next(); append_state($it); $it->next(); append_state($it); unset($it);
    } else {
        foreach(['rewind','valid','current','key','next'] as $failure) {
            echo 'fail:', $failure, "\n"; $it=new AppendIterator(); $input=new AppendObservedInput();
            AppendObservedInput::$failure=$failure;
            append_attempt(fn()=>$it->append($input));
            AppendObservedInput::$failure=''; append_state($it);
            AppendObservedInput::$failure=$failure; append_attempt(fn()=>$it->next());
            AppendObservedInput::$failure=''; append_state($it); unset($it,$input);
        }
    }
    break;
case 'nested':
    $outer=new AppendIterator(); $inner=new AppendIterator(); $outer->append($inner); append_state($outer);
    $inner->append(new ArrayIterator(['inside'=>'one'])); append_state($outer);
    $outer->append(new ArrayIterator(['outside'=>'two'])); append_state($outer);
    append_show(iterator_to_array($outer));
    break;
case 'reference':
    $scalar='before'; $array=['n'=>1]; $inner=new ArrayIterator(['ref'=>&$scalar,'copy'=>$array]);
    $it=new AppendIterator(); $it->append($inner); $scalar='after'; append_state($it);
    $copy=$it->current(); $copy='local'; append_show([$scalar,$it->current()]);
    $it->next(); $copy=$it->current(); $copy['n']=2; $array['n']=3;
    append_show([$it->current(),$inner->current(),$copy,$array]);
    break;
case 'forwarding':
    class AppendNamedInput extends ArrayIterator {
        function change(&$value, $suffix='!') { $value.=$suffix; return $this->current(); }
    }
    $it=new AppendIterator();
    append_attempt(fn()=>$it->absent());
    $it->append(new AppendNamedInput(['one'])); $it->append(new AppendNamedInput(['two']));
    $value='start'; append_show($it->change(suffix:'?',value:$value)); append_show($value);
    $it->next(); append_show($it->change($value)); append_show($value);
    $it->next(); append_attempt(fn()=>$it->change($value));
    break;
case 'generator':
    function appended_generator() { yield 'k'=>'once'; }
    $g=appended_generator(); foreach($g as $value) {}
    $it=new AppendIterator(); append_attempt(fn()=>$it->append($g)); append_state($it);
    append_attempt(fn()=>$it->next()); append_state($it);
    $fresh=appended_generator(); $other=new AppendIterator(); $other->append($fresh); append_state($other);
    $other->next(); append_state($other); append_attempt(fn()=>$other->rewind()); append_state($other);
    break;
case 'invalid':
    $it=new AppendIterator();
    foreach([null,false,3,[],new stdClass()] as $value) append_attempt(fn()=>$it->append($value));
    append_state($it);
    class AppendUninitialized extends AppendIterator { function __construct() {} }
    $empty=new AppendUninitialized();
    foreach(['rewind','next','valid','current','key','getIteratorIndex','getArrayIterator','getInnerIterator'] as $method) append_attempt(fn()=>$empty->$method());
    append_attempt(fn()=>$empty->append(new ArrayIterator([])));
    break;
case 'lifetime':
    class AppendRetiredInput extends ArrayIterator { function __destruct() { echo "inner retired\n"; } }
    class AppendRetiredOwner extends AppendIterator { function __destruct() { echo "owner retired\n"; } }
    $it=new AppendRetiredOwner(); $inner=new AppendRetiredInput(['kept']); $it->append($inner);
    $list=$it->getArrayIterator(); unset($inner,$it); echo "list retained\n"; unset($list); echo "done\n";
    break;
case 'cycle':
    class AppendCycleOwner extends AppendIterator { function __destruct() { echo "cycle owner retired\n"; } }
    class AppendCycleInput extends ArrayIterator { public $owner; function __destruct() { echo "cycle input retired\n"; } }
    $it=new AppendCycleOwner(); $input=new AppendCycleInput(['value']); $input->owner=$it; $it->append($input);
    unset($input,$it); gc_collect_cycles(); echo "after collection\n";
    break;
case 'arity':
    function append_trace($call) {
        try { $call(); } catch(Throwable $e) {
            $frame=$e->getTrace()[0];
            append_show([$e::class,$e->getMessage(),$frame['function'],$frame['class']??null,$frame['type']??null,$frame['args']??null]);
        }
    }
    foreach([false,true] as $ignore) {
        ini_set('zend.exception_ignore_args',$ignore?'1':'0');
        append_trace(fn()=>new AppendIterator(null));
        $it=new ArrayIterator([]); append_trace(fn()=>$it->count('extra'));
        $length='strlen'; append_trace(fn()=>$length('value','extra'));
    }
    function append_user_arity() { return func_get_args(); }
    append_show(append_user_arity('allowed','extra'));
    break;
case 'reentry':
    class ReenteredAppendInput extends ArrayIterator {
        public $owner;
        public $once=true;
        public $event;
        function reenter($event) {
            echo $event,"\n";
            if($this->once && $this->event===$event) {
                $this->once=false;
                $this->owner->append(new ArrayIterator(['nested'=>'added']));
                echo 'appended:', $this->owner->getIteratorIndex(),"\n";
            }
        }
        function rewind(): void { $this->reenter('rewind'); parent::rewind(); }
        function current(): mixed { $this->reenter('current'); return parent::current(); }
    }
    foreach(['current','rewind'] as $event) {
        echo 'event:',$event,"\n";
        $it=new AppendIterator(); $input=new ReenteredAppendInput(['first'=>'base']);
        $input->owner=$it; $input->event=$event; $it->append($input);
        for($i=0;$it->valid() && $i<4;++$i,$it->next()) echo $it->key(),':',$it->current(),"\n";
        unset($input->owner,$input,$it);
    }
    break;
}
