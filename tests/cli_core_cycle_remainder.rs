use std::process::Command;

#[test]
fn heap_nested_resurrection_and_throw() {
    assert_cycle_contract(
        r###"class Record { static $saved; function __construct(public $name, public $queue) {} function __destruct() {
 echo $this->name,':',$this->queue->count(),'|';
 if ($this->name==='first') {self::$saved=$this->queue->current();throw new Exception("saved");}
} }
$queue=new SplPriorityQueue;
$queue->insert([new Record('first',$queue),new Record('second',$queue)],2);
try{$queue->next();}catch(Exception $error){echo $error->getMessage(),'|';}echo 'next|';Record::$saved=null;echo 'saved|';"###,
        "first:1|second:1|saved|next|saved|",
    );
}

#[test]
fn heap_direct_final() {
    assert_cycle_contract(
        r###"class Record { function __construct(public $queue) {} function __destruct() {
 echo 'drop:', $this->queue->count(), '|';
 try { $this->queue->insert('later', 9); echo 'open|'; }
 catch (RuntimeException $error) { echo 'locked|'; }
} }
$queue=new SplPriorityQueue;$value=new Record($queue);
$entry=$value;

$queue->insert($entry,2);unset($value,$entry);$queue->next();echo 'next|';

echo 'count:',$queue->count(),'|';"###,
        "drop:1|locked|next|count:0|",
    );
}

#[test]
fn heap_direct_shared() {
    assert_cycle_contract(
        r###"class Record { function __construct(public $queue) {} function __destruct() {
 echo 'drop:', $this->queue->count(), '|';
 try { $this->queue->insert('later', 9); echo 'open|'; }
 catch (RuntimeException $error) { echo 'locked|'; }
} }
$queue=new SplPriorityQueue;$value=new Record($queue);
$entry=$value;
$saved=$entry;
$queue->insert($entry,2);unset($value,$entry);$queue->next();echo 'next|';
unset($saved);echo 'saved|';
echo 'count:',$queue->count(),'|';"###,
        "next|drop:0|open|saved|count:1|",
    );
}

#[test]
fn heap_nested_final() {
    assert_cycle_contract(
        r###"class Record { function __construct(public $queue) {} function __destruct() {
 echo 'drop:', $this->queue->count(), '|';
 try { $this->queue->insert('later', 9); echo 'open|'; }
 catch (RuntimeException $error) { echo 'locked|'; }
} }
$queue=new SplPriorityQueue;$value=new Record($queue);
$entry=[[$value]];

$queue->insert($entry,2);unset($value,$entry);$queue->next();echo 'next|';

echo 'count:',$queue->count(),'|';"###,
        "drop:1|locked|next|count:0|",
    );
}

#[test]
fn heap_nested_shared() {
    assert_cycle_contract(
        r###"class Record { function __construct(public $queue) {} function __destruct() {
 echo 'drop:', $this->queue->count(), '|';
 try { $this->queue->insert('later', 9); echo 'open|'; }
 catch (RuntimeException $error) { echo 'locked|'; }
} }
$queue=new SplPriorityQueue;$value=new Record($queue);
$entry=[[$value]];
$saved=$entry;
$queue->insert($entry,2);unset($value,$entry);$queue->next();echo 'next|';
unset($saved);echo 'saved|';
echo 'count:',$queue->count(),'|';"###,
        "next|drop:0|open|saved|count:1|",
    );
}

#[test]
fn heap_nested_resurrected_container() {
    assert_cycle_contract(
        r###"class Record { static $saved; function __construct(public $name, public $queue) {} function __destruct() {
 echo $this->name,':',$this->queue->count(),'|';
 if ($this->name==='first') self::$saved=$this->queue->current();
} }
$queue=new SplPriorityQueue;
$queue->insert([new Record('first',$queue),new Record('second',$queue)],2);
$queue->next();echo 'next|';Record::$saved=null;echo 'saved|';"###,
        "first:1|second:1|next|saved|",
    );
}

#[test]
fn heap_nested_throwing_destructor() {
    assert_cycle_contract(
        r###"class Record { function __construct(public $name, public $queue) {} function __destruct() {
 echo $this->name,':',$this->queue->count(),'|';if($this->name==='first')throw new Exception('retire');
} }
$queue=new SplPriorityQueue;$queue->insert([new Record('first',$queue),new Record('second',$queue)],2);
try{$queue->next();}catch(Exception $error){echo $error->getMessage(),'|';}echo 'count:',$queue->count(),'|';"###,
        "first:1|second:1|retire|count:0|",
    );
}

#[test]
fn generator_local_direct_first_final() {
    assert_cycle_contract(
        r###"class Payload { function __construct(public $name){}function __destruct(){echo $this->name,'|';}function callback(){return function(){return $this;};} }
function pending(...$values){}
$loop=null;$keep=new Payload('direct');$bound=(new Payload('bound'))->callback();
$loop=(function($direct,$callback)use(&$loop){pending($loop,$callback,yield 'ready');})($keep,$bound);
unset($bound);unset($keep);echo $loop->current(),'|';echo 'rooted:',gc_collect_cycles(),'|';
$loop=null;echo 'collect|';gc_collect_cycles();echo 'done|';;"###,
        "ready|rooted:0|collect|direct|bound|done|",
    );
}

#[test]
fn generator_local_direct_first_retained() {
    assert_cycle_contract(
        r###"class Payload { function __construct(public $name){}function __destruct(){echo $this->name,'|';}function callback(){return function(){return $this;};} }
function pending(...$values){}
$loop=null;$keep=new Payload('direct');$bound=(new Payload('bound'))->callback();
$loop=(function($direct,$callback)use(&$loop){pending($loop,$callback,yield 'ready');})($keep,$bound);
unset($bound);echo $loop->current(),'|';echo 'rooted:',gc_collect_cycles(),'|';
$loop=null;echo 'collect|';gc_collect_cycles();echo 'done|';unset($keep);echo 'keep|';"###,
        "ready|rooted:0|collect|bound|done|direct|keep|",
    );
}

#[test]
fn generator_local_callback_first_final() {
    assert_cycle_contract(
        r###"class Payload { function __construct(public $name){}function __destruct(){echo $this->name,'|';}function callback(){return function(){return $this;};} }
function pending(...$values){}
$loop=null;$keep=new Payload('direct');$bound=(new Payload('bound'))->callback();
$loop=(function($callback,$direct)use(&$loop){pending($loop,$callback,yield 'ready');})($bound,$keep);
unset($bound);unset($keep);echo $loop->current(),'|';echo 'rooted:',gc_collect_cycles(),'|';
$loop=null;echo 'collect|';gc_collect_cycles();echo 'done|';;"###,
        "ready|rooted:0|collect|direct|bound|done|",
    );
}

#[test]
fn generator_local_callback_first_retained() {
    assert_cycle_contract(
        r###"class Payload { function __construct(public $name){}function __destruct(){echo $this->name,'|';}function callback(){return function(){return $this;};} }
function pending(...$values){}
$loop=null;$keep=new Payload('direct');$bound=(new Payload('bound'))->callback();
$loop=(function($callback,$direct)use(&$loop){pending($loop,$callback,yield 'ready');})($bound,$keep);
unset($bound);echo $loop->current(),'|';echo 'rooted:',gc_collect_cycles(),'|';
$loop=null;echo 'collect|';gc_collect_cycles();echo 'done|';unset($keep);echo 'keep|';"###,
        "ready|rooted:0|collect|bound|done|direct|keep|",
    );
}

#[test]
fn static_container_shutdown_fixed_point() {
    assert_cycle_contract(
        r###"class Pending { static $remaining=3; static $queue=[]; function __destruct(){echo self::$remaining,'|';if(self::$remaining>0){--self::$remaining;self::$queue[]=new Pending;}} } new Pending;echo 'end|';"###,
        "3|end|2|1|0|",
    );
}

#[test]
fn static_nested_container_shutdown() {
    assert_cycle_contract(
        r###"class Notice { function __destruct(){echo 'drop|';} }class Store {static $box;} Store::$box=['nested'=>[new Notice]];echo 'end|';"###,
        "end|drop|",
    );
}

#[test]
fn shutdown_error_handler_generator() {
    assert_cycle_contract(
        r###"function stream(){try{yield 4;}finally{echo 'finally|';}}$stream=stream();echo $stream->current(),'|';set_error_handler(function()use($stream){return true;});echo 'end|';"###,
        "4|end|finally|",
    );
}

#[test]
fn shutdown_handler_generator_after_unset() {
    assert_cycle_contract(
        r###"function stream(){try{yield 4;}finally{echo 'finally|';}}$stream=stream();echo $stream->current(),'|';set_error_handler(function()use($stream){return true;});unset($stream);echo 'end|';"###,
        "4|end|finally|",
    );
}

#[test]
fn shutdown_exception_handler_generator() {
    assert_cycle_contract(
        r###"function stream(){try{yield 4;}finally{echo 'finally|';}}$stream=stream();echo $stream->current(),'|';set_exception_handler(function($error)use($stream){});echo 'end|';"###,
        "4|end|finally|",
    );
}

#[test]
fn replaced_handler_keeps_a_live_alias() {
    assert_cycle_contract(
        r###"function stream(){try{yield 4;}finally{echo 'finally|';}}$stream=stream();echo $stream->current(),'|';$handler=function()use($stream){return true;};set_error_handler($handler);restore_error_handler();echo 'restored|';unset($handler);echo 'alias|';unset($stream);echo 'end|';"###,
        "4|restored|alias|finally|end|",
    );
}

#[test]
fn shutdown_handler_reads_global_array() {
    assert_cycle_contract(
        r###"$_GET=['answer'=>17];function source(){try{yield 6;}finally{echo $_GET['answer'],'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|17|",
    );
}

#[test]
fn shutdown_handler_resurrection() {
    assert_cycle_contract(
        r###"class Notice{function __destruct(){global $saved;echo 'drop|';$saved=$this;}}$root=new Notice;set_error_handler(function()use($root){});echo 'end|';"###,
        "end|drop|",
    );
}

#[test]
fn shutdown_current_4_literal() {
    assert_cycle_contract(
        r###"function source(){try{yield 4;}finally{echo 'closed|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|closed|",
    );
}

#[test]
fn shutdown_current_4_global() {
    assert_cycle_contract(
        r###"function source(){try{yield 4;}finally{echo 'keys:',count($_GET),'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|keys:0|",
    );
}

#[test]
fn shutdown_current_4_dump() {
    assert_cycle_contract(
        r###"function source(){try{yield 4;}finally{var_dump($_GET);}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|array(0) {\n}\n",
    );
}

#[test]
fn shutdown_current_null_literal() {
    assert_cycle_contract(
        r###"function source(){try{yield null;}finally{echo 'closed|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|closed|",
    );
}

#[test]
fn shutdown_current_null_global() {
    assert_cycle_contract(
        r###"function source(){try{yield null;}finally{echo 'keys:',count($_GET),'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|keys:0|",
    );
}

#[test]
fn shutdown_current_null_dump() {
    assert_cycle_contract(
        r###"function source(){try{yield null;}finally{var_dump($_GET);}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|array(0) {\n}\n",
    );
}

#[test]
fn shutdown_rewind_4_literal() {
    assert_cycle_contract(
        r###"function source(){try{yield 4;}finally{echo 'closed|';}}$it=source();$it->rewind();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|closed|",
    );
}

#[test]
fn shutdown_rewind_4_global() {
    assert_cycle_contract(
        r###"function source(){try{yield 4;}finally{echo 'keys:',count($_GET),'|';}}$it=source();$it->rewind();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|keys:0|",
    );
}

#[test]
fn shutdown_rewind_4_dump() {
    assert_cycle_contract(
        r###"function source(){try{yield 4;}finally{var_dump($_GET);}}$it=source();$it->rewind();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|array(0) {\n}\n",
    );
}

#[test]
fn shutdown_rewind_null_literal() {
    assert_cycle_contract(
        r###"function source(){try{yield null;}finally{echo 'closed|';}}$it=source();$it->rewind();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|closed|",
    );
}

#[test]
fn shutdown_rewind_null_global() {
    assert_cycle_contract(
        r###"function source(){try{yield null;}finally{echo 'keys:',count($_GET),'|';}}$it=source();$it->rewind();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|keys:0|",
    );
}

#[test]
fn shutdown_rewind_null_dump() {
    assert_cycle_contract(
        r###"function source(){try{yield null;}finally{var_dump($_GET);}}$it=source();$it->rewind();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|array(0) {\n}\n",
    );
}

#[test]
fn generator_global_publication() {
    assert_cycle_contract(
        r###"function fill(){global $bag;$bag[]=7;yield 11;}$it=fill();echo $it->current(),'|';unset($it);echo $bag[0],'|';"###,
        "11|7|",
    );
}

#[test]
fn unlink_pair() {
    assert_cycle_contract(
        r###"class Pair { public $next; function __construct(public $tag){} function __destruct(){echo $this->tag,'|';unset($this->next);} }$left=new Pair('left');$right=new Pair('right');$left->next=$right;$right->next=$left;unset($left,$right);echo 'collected:',gc_collect_cycles(),'|';"###,
        "collected:left|right|1|",
    );
}

#[test]
fn unlink_three() {
    assert_cycle_contract(
        r###"class Pair { public $next; function __construct(public $tag){} function __destruct(){echo $this->tag,'|';unset($this->next);} }$a=new Pair('a');$b=new Pair('b');$c=new Pair('c');$a->next=$b;$b->next=$c;$c->next=$a;unset($a,$b,$c);echo 'collected:',gc_collect_cycles(),'|';"###,
        "collected:a|b|c|1|",
    );
}

#[test]
fn unlink_live_alias() {
    assert_cycle_contract(
        r###"class Pair { public $next; function __construct(public $tag){} function __destruct(){echo $this->tag,'|';unset($this->next);} }$a=new Pair('a');$b=new Pair('b');$a->next=$b;$b->next=$a;$saved=$b;unset($a,$b);echo 'live:',gc_collect_cycles(),'|';unset($saved);echo 'dead:',gc_collect_cycles(),'|';"###,
        "live:0|dead:b|a|1|",
    );
}

#[test]
fn tree_method_edges() {
    assert_cycle_contract(
        r###"class Vertex { public $edges=[]; public $parent=null; function __construct(public $tag){} function join($child){$child->parent=$this;$this->edges[]=$child;} function __destruct(){echo $this->tag,'|';unset($this->edges,$this->parent);} }$root=new Vertex('root');$first=new Vertex('first');$last=new Vertex('last');$root->join($first);$root->join($last);unset($root,$first,$last);echo 'collected:',gc_collect_cycles(),'|';"###,
        "collected:root|first|last|1|",
    );
}

#[test]
fn tree_direct_edges() {
    assert_cycle_contract(
        r###"class Vertex { public $edges=[]; public $parent=null; function __construct(public $tag){} function join($child){$child->parent=$this;$this->edges[]=$child;} function __destruct(){echo $this->tag,'|';unset($this->edges,$this->parent);} }$root=new Vertex('root');$first=new Vertex('first');$last=new Vertex('last');$first->parent=$root;$last->parent=$root;$root->edges=[$first,$last];unset($root,$first,$last);echo 'collected:',gc_collect_cycles(),'|';"###,
        "collected:root|first|last|1|",
    );
}

#[test]
fn tree_function_scope() {
    assert_cycle_contract(
        r###"class Vertex { public $edges=[]; public $parent=null; function __construct(public $tag){} function join($child){$child->parent=$this;$this->edges[]=$child;} function __destruct(){echo $this->tag,'|';unset($this->edges,$this->parent);} }function make(){ $root=new Vertex('root');$first=new Vertex('first');$last=new Vertex('last');$root->join($first);$root->join($last);}make();echo 'collected:',gc_collect_cycles(),'|';"###,
        "collected:root|first|last|1|",
    );
}

#[test]
fn tree_weak_witness() {
    assert_cycle_contract(
        r###"class Vertex { public $edges=[]; public $parent=null; function __construct(public $tag){} function join($child){$child->parent=$this;$this->edges[]=$child;} function __destruct(){echo $this->tag,'|';unset($this->edges,$this->parent);} }$root=new Vertex('root');$child=new Vertex('child');$root->join($child);$weak=WeakReference::create($root);unset($root,$child);echo 'collected:',gc_collect_cycles(),'|gone:',(int)($weak->get()===null),'|';"###,
        "collected:root|child|1|gone:1|",
    );
}

#[test]
fn tree_null_edges() {
    assert_cycle_contract(
        r###"class Vertex { public $edges=null; public $parent=null; function __construct(public $tag){} function join($child){$child->parent=$this;$this->edges[]=$child;} function __destruct(){echo $this->tag,'|';unset($this->edges,$this->parent);} }$root=new Vertex('root');$first=new Vertex('first');$last=new Vertex('last');$root->join($first);$root->join($last);unset($root,$first,$last);echo 'collected:',gc_collect_cycles(),'|';"###,
        "collected:root|first|last|1|",
    );
}

#[test]
fn tree_uninitialized_edges() {
    assert_cycle_contract(
        r###"class Vertex { public $edges; public $parent=null; function __construct(public $tag){} function join($child){$child->parent=$this;$this->edges[]=$child;} function __destruct(){echo $this->tag,'|';unset($this->edges,$this->parent);} }$root=new Vertex('root');$first=new Vertex('first');$last=new Vertex('last');$root->join($first);$root->join($last);unset($root,$first,$last);echo 'collected:',gc_collect_cycles(),'|';"###,
        "collected:root|first|last|1|",
    );
}

#[test]
fn tree_declared_name() {
    assert_cycle_contract(
        r###"class Vertex { public $edges=[]; public $parent=null; public $tag;function __construct($tag){$this->tag=$tag;} function join($child){$child->parent=$this;$this->edges[]=$child;} function __destruct(){echo $this->tag,'|';unset($this->edges,$this->parent);} }$root=new Vertex('root');$first=new Vertex('first');$last=new Vertex('last');$root->join($first);$root->join($last);unset($root,$first,$last);echo 'collected:',gc_collect_cycles(),'|';"###,
        "collected:root|first|last|1|",
    );
}

#[test]
fn property_reference_inline_child() {
    assert_cycle_contract(
        r###"$root=new stdClass;$root->edges=[];$root->edges['leaf']=new stdClass;$root->edges['leaf']->parent=$root;$root->edges['self'] =& $root->edges;echo 'live:',gc_collect_cycles(),'|';unset($root);echo 'dead:',gc_collect_cycles(),'|';"###,
        "live:0|dead:3|",
    );
}

#[test]
fn property_reference_cycle() {
    assert_cycle_contract(
        r###"$root=new stdClass;$leaf=new stdClass;$root->edges=[$leaf];$leaf->parent=$root;$root->edges['self'] =& $root->edges;unset($leaf);echo 'live:',gc_collect_cycles(),'|';unset($root);echo 'dead:',gc_collect_cycles(),'|';"###,
        "live:0|dead:3|",
    );
}

#[test]
fn property_reference_no_prescan() {
    assert_cycle_contract(
        r###"$root=new stdClass;$leaf=new stdClass;$root->edges=[$leaf];$leaf->parent=$root;$root->edges['self'] =& $root->edges;unset($root,$leaf);echo 'dead:',gc_collect_cycles(),'|';"###,
        "dead:3|",
    );
}

#[test]
fn property_cycle_no_reference() {
    assert_cycle_contract(
        r###"$root=new stdClass;$leaf=new stdClass;$root->edges=[$leaf];$leaf->parent=$root;unset($leaf);echo 'live:',gc_collect_cycles(),'|';unset($root);echo 'dead:',gc_collect_cycles(),'|';"###,
        "live:0|dead:3|",
    );
}

#[test]
fn array_children_small() {
    assert_cycle_contract(
        r###"$forest=[];for($i=0;$i<7;$i++){$forest[$i]=[];$forest[$i]['self'] =& $forest[$i];}echo 'live:',gc_collect_cycles(),'|';unset($forest);echo 'dead:',gc_collect_cycles(),'|';"###,
        "live:0|dead:7|",
    );
}

#[test]
fn delegated_finally_cycle() {
    assert_cycle_contract(
        r###"function leaf(){global $links;try{yield 9;}finally{echo 'finally:',count($links),'|';}}function wrap($depth){global $links;$next=$depth?wrap($depth-1):leaf();$links[]=$next;yield from $next;}$outer=wrap(1);$links=[$outer];echo $outer->current(),'|';unset($outer,$links);echo 'collect|';gc_collect_cycles();echo 'end|';"###,
        "9|collect|finally:3|end|",
    );
}

#[test]
fn promotion_append_null() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=null;$box[]=new Watch;echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn promotion_append_array() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=[];$box[]=new Watch;echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn promotion_dimension_null() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=null;$box['item']=new Watch;echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn promotion_dimension_array() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=[];$box['item']=new Watch;echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn promotion_reference_append_null() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=null;$item=new Watch;$box[] =& $item;unset($item);echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn promotion_reference_append_array() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=[];$item=new Watch;$box[] =& $item;unset($item);echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn promotion_reference_dimension_null() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=null;$item=new Watch;$box['item'] =& $item;unset($item);echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn promotion_reference_dimension_array() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=[];$item=new Watch;$box['item'] =& $item;unset($item);echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn cyclic_child_waits_for_gc() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}$tree=['branch'=>['watch'=>new Watch]];$tree['branch']['loop'] =& $tree['branch'];echo 'before|';unset($tree);echo 'unset|';echo gc_collect_cycles(),'|after|';"###,
        "before|unset|drop|1|after|",
    );
}

#[test]
fn cyclic_destructor_requests_second_pass() {
    assert_cycle_contract(
        r###"class Ring{public $self;function __destruct(){echo 'drop|';}}$v=new Ring;$v->self=$v;unset($v);echo gc_collect_cycles(),':',gc_status()['runs'],'|';"###,
        "drop|1:2|",
    );
}

#[test]
fn plain_leaf_is_counted() {
    assert_cycle_contract(
        r###"class Leaf{}$v=new stdClass;$v->self=$v;$v->leaf=new Leaf;unset($v);echo gc_collect_cycles(),':',gc_status()['runs'],'|';"###,
        "2:1|",
    );
}

#[test]
fn acyclic_destructor_uses_one_pass() {
    assert_cycle_contract(
        r###"class Leaf{function __destruct(){echo 'drop|';}}$v=new stdClass;$v->self=$v;$v->leaf=new Leaf;unset($v);echo gc_collect_cycles(),':',gc_status()['runs'],'|';"###,
        "drop|1:1|",
    );
}

#[test]
fn callback_root_is_visited_by_rerun() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){new Record;}}$v=new stdClass;$v->self=$v;$v->leaf=new Leaf;unset($v);echo gc_collect_cycles(),':',gc_status()['runs'],':',gc_status()['roots'],'|';"###,
        "1:2:0|",
    );
}

#[test]
fn local_array_preserves_exported_reference() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){global $alias;$box=[new Watch];$alias =& $box;echo 'body|';}fill();echo 'after|';unset($alias);echo 'end|';"###,
        "body|drop|after|end|",
    );
}

#[test]
fn local_array_preserves_exported_cow_owner() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){global $saved;$box=[new Watch];$saved=$box;echo 'body|';}fill();echo 'after|';unset($saved);echo 'end|';"###,
        "body|after|drop|end|",
    );
}

#[test]
fn local_array_destructor_resurrection() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){global $saved;$saved=$this;echo 'drop|';}}function fill(){$box=[new Watch];echo 'body|';}fill();echo 'after:',get_class($saved),'|';unset($saved);echo 'end|';"###,
        "body|drop|after:Watch|end|",
    );
}

#[test]
fn local_array_destructor_throw() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){throw new Exception('drop');}}function fill(){$box=[new Watch];echo 'body|';}try{fill();echo 'bad|';}catch(Exception $e){echo $e->getMessage(),'|';}echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn local_array_cycle_waits_for_collection() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=[new Watch];$box['self'] =& $box;echo 'body|';}fill();echo 'after|',gc_collect_cycles(),'|end|';"###,
        "body|after|drop|1|end|",
    );
}

#[test]
fn shared_local_container_releases_once() {
    assert_cycle_contract(
        r###"class Watch{function __destruct(){echo 'drop|';}}function fill(){$box=[new Watch];$other=$box;$alias =& $box;echo 'body|';}fill();echo 'after|';"###,
        "body|drop|after|",
    );
}

#[test]
fn temporary_key_provenance() {
    assert_cycle_contract(
        r###"$forest=[];$key="\xff";$forest[$key]=['marker'=>3];$forest[$key]['self'] =& $forest[$key];echo gc_status()['roots'],'|';unset($forest);echo gc_collect_cycles(),'|';"###,
        "0|1|",
    );
}

#[test]
fn forest_7_false() {
    assert_cycle_contract(
        r###"$forest=[];for($i=0;$i<7;$i++){$forest[$i]=['marker'=>$i];$forest[$i]['loop'] =& $forest[$i];}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';unset($forest);$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:0:0:10001|0:7:0:10001|collect:7|1:0:7:10001|",
    );
}

#[test]
fn forest_7_true() {
    assert_cycle_contract(
        r###"$forest=[];for($i=0;$i<7;$i++){$forest[$i]=['marker'=>$i];$forest[$i]['loop'] =& $forest[$i];}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'scan:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';unset($forest);$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:0:0:10001|scan:0|0:0:0:10001|0:7:0:10001|collect:7|1:0:7:10001|",
    );
}

#[test]
fn forest_9999_false() {
    assert_cycle_contract(
        r###"$forest=[];for($i=0;$i<9999;$i++){$forest[$i]=['marker'=>$i];$forest[$i]['loop'] =& $forest[$i];}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';unset($forest);$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:0:0:10001|0:9999:0:10001|collect:9999|1:0:9999:10001|",
    );
}

#[test]
fn forest_9999_true() {
    assert_cycle_contract(
        r###"$forest=[];for($i=0;$i<9999;$i++){$forest[$i]=['marker'=>$i];$forest[$i]['loop'] =& $forest[$i];}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'scan:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';unset($forest);$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:0:0:10001|scan:0|0:0:0:10001|0:9999:0:10001|collect:9999|1:0:9999:10001|",
    );
}

#[test]
fn forest_10001_false() {
    assert_cycle_contract(
        r###"$forest=[];for($i=0;$i<10001;$i++){$forest[$i]=['marker'=>$i];$forest[$i]['loop'] =& $forest[$i];}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';unset($forest);$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:0:0:10001|1:1:10000:10001|collect:1|2:0:10001:10001|",
    );
}

#[test]
fn forest_10001_true() {
    assert_cycle_contract(
        r###"$forest=[];for($i=0;$i<10001;$i++){$forest[$i]=['marker'=>$i];$forest[$i]['loop'] =& $forest[$i];}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'scan:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';unset($forest);$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:0:0:10001|scan:0|0:0:0:10001|1:1:10000:10001|collect:1|2:0:10001:10001|",
    );
}

#[test]
fn leaf_plain_1() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<1;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:2:0:10001|collect:2|1:0:2:10001|",
    );
}

#[test]
fn leaf_plain_7() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<7;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:14:0:10001|collect:14|1:0:14:10001|",
    );
}

#[test]
fn leaf_plain_4999() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<4999;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:9998:0:10001|collect:9998|1:0:9998:10001|",
    );
}

#[test]
fn leaf_plain_5000() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<5000;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:10000:0:10001|collect:10000|1:0:10000:10001|",
    );
}

#[test]
fn leaf_plain_5001() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<5001;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "1:2:10000:10001|collect:2|2:0:10002:10001|",
    );
}

#[test]
fn leaf_plain_10001() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<10001;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "2:2:20000:10001|collect:2|3:0:20002:10001|",
    );
}

#[test]
fn leaf_destructor_1() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<1;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:2:0:10001|collect:1|1:0:1:10001|",
    );
}

#[test]
fn leaf_destructor_7() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<7;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:14:0:10001|collect:7|1:0:7:10001|",
    );
}

#[test]
fn leaf_destructor_4999() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<4999;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:9998:0:10001|collect:4999|1:0:4999:10001|",
    );
}

#[test]
fn leaf_destructor_5000() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<5000;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:10000:0:10001|collect:5000|1:0:5000:10001|",
    );
}

#[test]
fn leaf_destructor_5001() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<5001;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "1:2:5000:10001|collect:1|2:0:5001:10001|",
    );
}

#[test]
fn leaf_destructor_10001() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<10001;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "2:2:10000:10001|collect:1|3:0:10001:10001|",
    );
}

#[test]
fn leaf_allocating_1() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){new Record;}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<1;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:2:0:10001|collect:1|2:0:1:10001|",
    );
}

#[test]
fn leaf_allocating_7() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){new Record;}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<7;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:14:0:10001|collect:7|2:0:7:10001|",
    );
}

#[test]
fn leaf_allocating_4999() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){new Record;}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<4999;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:9998:0:10001|collect:4999|2:0:4999:10001|",
    );
}

#[test]
fn leaf_allocating_5000() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){new Record;}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<5000;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "0:10000:0:10001|collect:5000|2:0:5000:10001|",
    );
}

#[test]
fn leaf_allocating_5001() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){new Record;}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<5001;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "2:2:5000:10001|collect:1|4:0:5001:10001|",
    );
}

#[test]
fn leaf_allocating_10001() {
    assert_cycle_contract(
        r###"class Record{static $last;function __construct(){self::$last=$this;}}class Leaf{function __destruct(){new Record;}}class Ring{public $loop;public $leaf;function __construct($leaf){$this->leaf=$leaf;$this->loop=$this;}}for($i=0;$i<10001;$i++){new Ring(new Leaf);}$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';echo 'collect:',gc_collect_cycles(),'|';$s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],':',$s['threshold'],'|';"###,
        "4:2:10000:10001|collect:1|6:0:10001:10001|",
    );
}

fn assert_cycle_contract(source: &str, expected: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0", "-r", source])
        .output()
        .expect("run core cycle contract");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn generator_promotion_tracks_the_slot_not_its_external_referent() {
    assert_cycle_contract(
        "$bag=null;function fill(&$bag){$bag[]=7;yield 11;}$it=fill($bag);echo $it->current(),'|';unset($it);echo $bag[0],'|';",
        "11|7|",
    );
}

#[test]
fn dimension_promotion_preserves_a_caller_owned_reference() {
    assert_cycle_contract(
        "function fill(&$bag){$bag['value']=13;}$bag=null;fill($bag);echo $bag['value'],'|';",
        "13|",
    );
}

#[test]
fn append_binding_promotion_preserves_a_caller_owned_reference() {
    assert_cycle_contract(
        "function fill(&$bag){$entry =& $bag[];$entry=17;}$bag=null;fill($bag);echo $bag[0],'|';",
        "17|",
    );
}

#[test]
fn dimension_binding_promotion_preserves_a_caller_owned_reference() {
    assert_cycle_contract(
        "function fill(&$bag){$entry =& $bag['value'];$entry=19;}$bag=null;fill($bag);echo $bag['value'],'|';",
        "19|",
    );
}

#[test]
fn promoted_property_append_releases_its_temporary_owner() {
    assert_cycle_contract(
        "$owner=new stdClass;$owner->bag=null;$owner->bag[]=$owner;$weak=WeakReference::create($owner);unset($owner);echo gc_collect_cycles(),'|',(int)($weak->get()===null),'|';",
        "2|1|",
    );
}

#[test]
fn initialized_property_append_keeps_its_existing_ownership() {
    assert_cycle_contract(
        "$owner=new stdClass;$owner->bag=[];$owner->bag[]=$owner;$weak=WeakReference::create($owner);unset($owner);echo gc_collect_cycles(),'|',(int)($weak->get()===null),'|';",
        "2|1|",
    );
}

#[test]
fn promoted_property_dimension_releases_its_temporary_owner() {
    assert_cycle_contract(
        "$owner=new stdClass;$owner->bag=null;$owner->bag['back']=$owner;$weak=WeakReference::create($owner);unset($owner);echo gc_collect_cycles(),'|',(int)($weak->get()===null),'|';",
        "2|1|",
    );
}

#[test]
fn initialized_property_dimension_releases_its_temporary_owner() {
    assert_cycle_contract(
        "$owner=new stdClass;$owner->bag=[];$owner->bag['back']=$owner;$weak=WeakReference::create($owner);unset($owner);echo gc_collect_cycles(),'|',(int)($weak->get()===null),'|';",
        "2|1|",
    );
}

#[test]
fn inline_property_child_preserves_graph_edges_after_a_live_scan() {
    assert_cycle_contract(
        "$owner=new stdClass;$owner->bag=[];$owner->bag['leaf']=new stdClass;$owner->bag['leaf']->back=$owner;$owner->bag['self'] =& $owner->bag;echo gc_collect_cycles(),'|';unset($owner);echo gc_collect_cycles(),'|';",
        "0|3|",
    );
}

#[test]
fn promoted_property_keeps_a_live_reference_alias() {
    assert_cycle_contract(
        "$owner=new stdClass;$owner->bag=null;$alias =& $owner->bag;$owner->bag[]=$owner;unset($owner);echo gc_collect_cycles(),'|';unset($alias);echo gc_collect_cycles(),'|';",
        "0|2|",
    );
}

#[test]
fn promoted_property_keeps_a_live_cow_owner() {
    assert_cycle_contract(
        "$owner=new stdClass;$owner->bag=null;$owner->bag[]=$owner;$copy=$owner->bag;unset($owner);echo gc_collect_cycles(),'|';unset($copy);echo gc_collect_cycles(),'|';",
        "0|2|",
    );
}

#[test]
fn promoted_property_reference_append_preserves_the_source_cell() {
    assert_cycle_contract(
        "$owner=new stdClass;$owner->bag=null;$owner->bag[] =& $owner;unset($owner);echo gc_collect_cycles(),'|';",
        "2|",
    );
}

#[test]
fn method_frame_retires_a_promoted_property_array() {
    assert_cycle_contract(
        "class Tree{public $bag;function add($value){$this->bag[]=$value;}}$owner=new Tree;$owner->add($owner);unset($owner);echo gc_collect_cycles(),'|';",
        "2|",
    );
}

#[test]
fn failed_key_does_not_promote_the_property() {
    assert_cycle_contract(
        "function stop(){throw new Exception('stop');}$owner=new stdClass;$owner->bag=null;try{$owner->bag[stop()]=$owner;}catch(Exception $e){echo $e->getMessage(),'|';}var_dump($owner->bag);unset($owner);echo gc_collect_cycles(),'|';",
        "stop|NULL\n0|",
    );
}
