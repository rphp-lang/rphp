mod common;

#[test]
fn immutable_empty_array_templates_do_not_admit_mutable_roots() {
    for (source, expected) in [
        (
            "<?php $a=[];$b=$a;unset($b);echo gc_status()['roots'];",
            "0",
        ),
        (
            "<?php $a=[1];$b=$a;unset($b);echo gc_status()['roots'];",
            "1",
        ),
        (
            "<?php $a=[1];unset($a[0]);$b=$a;unset($b);echo gc_status()['roots'];",
            "1",
        ),
        (
            "<?php class H{public $items=[];}$h=new H;$h->items=[2];echo gc_status()['roots'];",
            "0",
        ),
        (
            "<?php class H{public $items=[1];}$h=new H;$h->items=[2];echo gc_status()['roots'];",
            "1",
        ),
    ] {
        assert_eq!(common::run_php(source), expected, "{source}");
    }
}

#[test]
fn callback_registration_does_not_admit_argument_snapshot_roots() {
    for call in [
        "set_error_handler(function(){});",
        "set_error_handler(callback:function(){});",
        "$name='set_error_handler';$name(function(){});",
        "$callback=function(){};set_error_handler($callback);",
        "set_exception_handler(function(){});",
        "set_exception_handler(callback:function(){});",
    ] {
        let source = format!("<?php {call} echo gc_status()['roots'];");
        assert_eq!(common::run_php(&source), "0", "{call}");
    }
}

#[test]
fn actual_callback_invocation_and_php_aliases_keep_gc_admission() {
    for (source, expected) in [
        (
            "<?php $f=function($x){return $x;};array_map($f,[1]);echo gc_status()['roots'];",
            "1",
        ),
        (
            "<?php $f=function($x){return $x;};$f(1);echo gc_status()['roots'];",
            "1",
        ),
        (
            "<?php $f=function(){};set_error_handler($f);$alias=$f;unset($alias);echo gc_status()['roots'];",
            "1",
        ),
        (
            "<?php function keep($x){$GLOBALS['kept']=$x;}keep(function(){});echo gc_status()['roots'];",
            "1",
        ),
    ] {
        assert_eq!(common::run_php(source), expected, "{source}");
    }
}

#[test]
fn shutdown_uses_object_store_order_after_child_first_root_admission() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Family {public $parent=null;public $children=[];
function attach($child){$child->parent=$this;$this->children[]=$child;}
function __destruct(){echo spl_object_id($this),':',isset($this->parent->children)?'linked':'open','|';unset($this->children);}}
$root=new Family;$root->attach(new Family);$root->attach(new Family);
"#
        ),
        "1:open|2:open|3:open|"
    );
}

#[test]
fn property_reference_writes_retire_referents_but_unset_preserves_aliases() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Item {function __construct(public $name){}function __destruct(){echo $this->name,'|';}}
class Holder {public $slot;}
function clearSlot($holder){$holder->slot=null;}
for($i=0;$i<2;$i++){
    $value=new Item('direct'.$i);$alias=&$value;$holder=new Holder;
    $holder->slot=&$value;clearSlot($holder);
    echo $alias===null?'null|':'live|';unset($value,$alias,$holder);
}
$value=[new Item('nested')];$holder=(object)['slot'=>&$value];
$holder->slot=null;echo $value===null?'null|':'live|';unset($value,$holder);
$value=new Item('retained');$holder=new Holder;$holder->slot=&$value;
unset($holder->slot);echo 'alias-held|';unset($holder,$value);echo 'done';
"#
        ),
        "direct0|null|direct1|null|nested|null|alias-held|retained|done"
    );
}

#[test]
fn transferred_append_sources_keep_cow_aliases_and_failed_write_retirement() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Item {function __construct(public $name){}function __destruct(){echo $this->name,'|';}}
$items=null;$items[]=new Item('stored');echo count($items),'|';unset($items);
$items=[PHP_INT_MAX=>1];try{$items[]=new Item('overflow');}catch(Error $e){echo 'caught|';}
echo count($items),'|';$items=[];$saved=($items[]=new Item('shared'));
unset($items);echo 'held|';unset($saved);
$a=[1];$b=$a;$a[]=$a;echo count($a),':',count($a[1]),':',count($b),'|';
$a=null;$a[]=$a;var_dump($a[0]);
"#
        ),
        "1|stored|overflow|caught|1|held|shared|2:1:1|NULL\n"
    );
}

#[test]
fn append_reference_results_copy_the_target_and_errors_allow_reentrant_replacement() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Item {function __construct(public $name){}function __destruct(){echo $this->name,'|';}}
$value=new Item('original');function &source(){global $value;return $value;}
$items=[];$items[]=source();$value=new Item('later');echo count($items),'|';
unset($items,$value);
$items=false;set_error_handler(function()use(&$items){$items=[];echo 'handler|';});
$items[]=new Item('unwritten');echo count($items),'|';
"#
        ),
        "1|original|later|handler|unwritten|0|"
    );
}

#[test]
fn moved_weak_reference_results_do_not_enter_the_possible_root_buffer() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Item {public $self;function __construct(public $name){$this->self=$this;}
function __destruct(){echo $this->name,'|';}}
$refs=[];
$refs[]=WeakReference::create(new Item('first'));
$refs[]=WeakReference::create(new Item('second'));
$refs[]=WeakReference::create(new Item('third'));
echo gc_status()['roots'],'|';gc_collect_cycles();
foreach($refs as $ref){echo $ref->get()===null?'cleared|':'live|';}
"#
        ),
        "3|first|second|third|cleared|cleared|cleared|"
    );
}

#[test]
fn releasing_a_real_weak_holder_alias_still_admits_a_root() {
    assert_eq!(
        common::run_php(
            r#"<?php
$target=new stdClass;
$ref=WeakReference::create($target);$alias=$ref;unset($alias);
unset($target);
echo gc_status()['roots'],'|';
echo $ref->get()===null?'cleared|':'live|';
echo gc_status()['roots'],'|';
"#
        ),
        "1|cleared|1|"
    );
}

#[test]
fn empty_and_scalar_reference_captures_do_not_reorder_later_cycles() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Item {public $self;function __construct(public $name){$this->self=$this;}
function __destruct(){echo $this->name,'|';}}
$refs=[];$scalar=7;
$fiber=new Fiber(function()use(&$refs,&$scalar){
$refs[]=WeakReference::create(new Item('first'));
$refs[]=WeakReference::create(new Item('second'));
$refs[]=WeakReference::create(new Item('third'));
echo gc_status()['roots'],'|';gc_collect_cycles();
});$fiber->start();
"#
        ),
        "4|third|first|second|"
    );
}

#[test]
fn retained_gc_destructor_fibers_finish_after_shutdown_callbacks() {
    assert_eq!(
        common::run_php(
            r#"<?php
register_shutdown_function(function(){echo 'shutdown|';});
class Item {public $self;function __construct(public $name){$this->self=$this;}
function __destruct(){echo $this->name,':enter|';$GLOBALS['saved'][]=Fiber::getCurrent();
try{Fiber::suspend();}finally{echo $this->name,':leave|';}}}
(new Fiber(function(){new Item('a');new Item('b');gc_collect_cycles();echo 'collected|';}))->start();
echo 'main|';
"#
        ),
        "b:enter|a:enter|collected|main|shutdown|b:leave|a:leave|"
    );
}

#[test]
fn live_roots_compact_from_the_tail_before_destructor_dispatch() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Item {public $self;function __construct(public $name){$this->self=$this;}
function __destruct(){echo $this->name,'|';}}
$kept=new stdClass;$alias=$kept;unset($alias);
new Item('first');new Item('second');new Item('third');
gc_collect_cycles();echo 'next|';
new Item('first');$alias=$kept;unset($alias);new Item('second');new Item('third');
gc_collect_cycles();
"#
        ),
        "third|first|second|next|first|third|second|"
    );
}

#[test]
fn fiber_collection_uses_the_same_live_root_compaction_as_main() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Item {public $self;function __construct(public $name){$this->self=$this;}
function __destruct(){echo $this->name,'|';}}
$fiber=new Fiber(function(){
new Item('first');new Item('second');new Item('third');
echo gc_status()['roots'],'|';gc_collect_cycles();echo 'next|';
new Item('first');new Item('second');new Item('third');gc_collect_cycles();
});$fiber->start();
"#
        ),
        "4|third|first|second|next|first|second|third|"
    );
}
