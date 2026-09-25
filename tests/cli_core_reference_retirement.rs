use std::process::Command;

#[test]
fn coalesce_keeps_nested_operand_release_after_private_reference_retirement() {
    assert_reference_retirement(
        r###"class Watch{function __destruct(){echo 'drop|';}}function choose(&$part,$unused){return 7;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$target=null;$target ??= choose($tree['branch'],[new Watch]);echo 'after|';unset($tree);echo gc_collect_cycles(),'|';"###,
        "drop|after|1|",
    );
}

#[test]
fn wide_frame_completed_reference_ranges() {
    let mut source = String::new();
    for index in 0..80 {
        source.push_str(&format!("$slot{index}={index};"));
    }
    source.push_str("$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];unset($tree);echo gc_collect_cycles(),'|',$slot0,':',$slot79,'|';");
    assert_reference_retirement(&source, "1|0:79|");
}

#[test]
fn wide_frame_failed_reference_ranges() {
    let mut source = String::new();
    for index in 0..80 {
        source.push_str(&format!("$slot{index}={index};"));
    }
    source.push_str("function reject(){throw new Exception('blocked');}function consume(&$value,$later){}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];try{consume($tree['branch'],reject());}catch(Exception $e){echo $e->getMessage(),'|';}unset($tree);echo gc_collect_cycles(),'|',$slot0,':',$slot79,'|';");
    assert_reference_retirement(&source, "blocked|1|0:79|");
}

#[test]
fn dynamic_scope_ordinary() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }function go($key){ $$key=new ScopeOwner('local'); echo 'body|'; } go('extra'); echo 'after|';"####,
        "body|drop:local|after|",
    );
}

#[test]
fn dynamic_scope_no_global_leak() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }function go($key){ $$key=new ScopeOwner('local'); $GLOBALS['weak']=WeakReference::create($$key); echo 'body|'; } go('extra'); echo 'global:',(int)isset($GLOBALS['extra']),'|gone:',(int)($weak->get()===null),'|';"####,
        "body|drop:local|global:0|gone:1|",
    );
}

#[test]
fn dynamic_scope_dynamic_order() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }function go($first,$second){$$first=new ScopeOwner('first');$$second=new ScopeOwner('second');echo 'body|';}go('zeta','alpha');echo 'after|';"####,
        "body|drop:first|drop:second|after|",
    );
}

#[test]
fn dynamic_scope_compiled_and_dynamic() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }function go($key){$$key=new ScopeOwner('dynamic');$local=new ScopeOwner('compiled');echo 'body|';}go('extra');echo 'after|';"####,
        "body|drop:compiled|drop:dynamic|after|",
    );
}

#[test]
fn dynamic_scope_shared_object() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }function go($key){$local=new ScopeOwner('shared');$$key=$local;echo 'body|';}go('extra');echo 'after|';"####,
        "body|drop:shared|after|",
    );
}

#[test]
fn dynamic_scope_exported_reference() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }function go($key){$$key=new ScopeOwner('shared');$GLOBALS['saved'] =& $$key; echo 'body|';}go('extra');echo 'alive|';unset($saved);echo 'after|';"####,
        "body|alive|drop:shared|after|",
    );
}

#[test]
fn dynamic_scope_local_reference() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }function go($key){$$key=new ScopeOwner('local');$local =& $$key;echo 'body|';}go('extra');echo 'after|';"####,
        "body|drop:local|after|",
    );
}

#[test]
fn dynamic_scope_dynamic_exception() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }function go($key){$$key=new ScopeOwner('local');throw new Exception('done');}try{go('extra');}catch(Exception $e){echo $e->getMessage(),'|';}"####,
        "drop:local|done|",
    );
}

#[test]
fn dynamic_scope_fiber() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }$f=new Fiber(function(){ $key='extra';$$key=new ScopeOwner('fiber');$GLOBALS['weak']=WeakReference::create($$key);Fiber::suspend('pause');echo 'alive:',(int)($$key===$GLOBALS['weak']->get()),'|';});echo $f->start(),'|';$f->resume();echo 'global:',(int)isset($GLOBALS['extra']),'|gone:',(int)($weak->get()===null),'|';"####,
        "pause|alive:1|drop:fiber|global:0|gone:1|",
    );
}

#[test]
fn dynamic_scope_plain_fiber() {
    assert_reference_retirement(
        r####"class ScopeOwner { function __construct(public $name) {} function __destruct(){echo 'drop:', $this->name, '|';} }$f=new Fiber(function(){ $key='extra';$$key=new ScopeOwner('fiber');Fiber::suspend('pause');echo 'body|';});echo $f->start(),'|';$f->resume();echo 'after|';"####,
        "pause|body|drop:fiber|after|",
    );
}

#[test]
fn dynamic_scope_caller_scope() {
    assert_reference_retirement(
        r####"$key='extra';$$key=11;$f=new Fiber(function(){ $key='extra';$$key=23;echo compact($key)[$key],'|';});$f->start();echo $$key,'|';"####,
        "23|11|",
    );
}

#[test]
fn dynamic_scope_dynamic_throwing_drop() {
    assert_reference_retirement(
        r####"class ScopeOwner{function __destruct(){echo 'drop|';throw new Exception('drop-error');}}function go($key){$$key=new ScopeOwner;echo 'body|';}try{go('extra');}catch(Exception $e){echo $e->getMessage(),'|';}"####,
        "body|drop|drop-error|",
    );
}

#[test]
fn boundary_assignment() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_compound() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=0;$answer+=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_concat() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer='';$answer.=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_array_set() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=[];$answer[0]=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_array_nested() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=[];$answer['one']['two']=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_array_append() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=[];$answer[]=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_property() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=new stdClass;$answer->value=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_property_array() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=new stdClass;$answer->values=[];$answer->values[0]=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_property_append() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=new stdClass;$answer->values=[];$answer->values[]=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_static_property() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];class Box{static $value;}Box::$value=access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_destructure() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];[$answer]=[access($tree['branch'])];unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_coalesce_miss() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer ??= access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_coalesce_hit() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=1;$answer ??= access($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_if_true() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];if(yes($tree['branch'])){echo 'body|';}unset($tree);echo gc_collect_cycles(),'|';"###,
        "body|1|",
    );
}

#[test]
fn boundary_if_false() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];if(no($tree['branch'])){echo 'wrong|';}else{echo 'else|';}unset($tree);echo gc_collect_cycles(),'|';"###,
        "else|1|",
    );
}

#[test]
fn boundary_while_false() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];while(no($tree['branch'])){echo 'wrong|';}unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_while_break() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];while(yes($tree['branch'])){echo 'body|';break;}unset($tree);echo gc_collect_cycles(),'|';"###,
        "body|1|",
    );
}

#[test]
fn boundary_do_while() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];do{echo 'body|';}while(no($tree['branch']));unset($tree);echo gc_collect_cycles(),'|';"###,
        "body|1|",
    );
}

#[test]
fn boundary_for_condition() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];for(;no($tree['branch']);){echo 'wrong|';}unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_for_preceding() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];for(;access($tree['branch']),false;){echo 'wrong|';}unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_for_update() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];for($i=0;$i<1;access($tree['branch']),++$i){echo 'body|';}unset($tree);echo gc_collect_cycles(),'|';"###,
        "body|1|",
    );
}

#[test]
fn boundary_condition_short_circuit() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];if(yes($tree['branch']) && no($tree['branch'])){echo 'wrong|';}unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_unset_key() {
    assert_reference_retirement(
        r###"function access(&$part){return 7;}function yes(&$part){return true;}function no(&$part){return false;}$tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$answer=[7=>'x'];unset($answer[access($tree['branch'])]);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_literal_no_mid_gc() {
    assert_reference_retirement(
        r###"$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];$aliases=[&$tree['branch']];unset($tree,$aliases);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_literal_function() {
    assert_reference_retirement(
        r###"function make(){$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];$aliases=[&$tree['branch']];}make();echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_literal_call() {
    assert_reference_retirement(
        r###"$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];function sink($v){}sink([&$tree['branch']]);unset($tree);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_literal_direct_ref() {
    assert_reference_retirement(
        r###"$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];$alias =& $tree['branch'];$aliases=[&$alias];unset($tree,$alias,$aliases);echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_literal_unset_element() {
    assert_reference_retirement(
        r###"$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];$aliases=[&$tree['branch']];unset($tree);echo gc_collect_cycles(),'|';unset($aliases[0]);echo gc_collect_cycles(),'|';unset($aliases);echo gc_collect_cycles(),'|';"###,
        "0|1|0|",
    );
}

#[test]
fn boundary_append_instead_of_literal() {
    assert_reference_retirement(
        r###"$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];$aliases=[];$aliases[] =& $tree['branch'];unset($tree);echo gc_collect_cycles(),'|';unset($aliases);echo gc_collect_cycles(),'|';"###,
        "0|1|",
    );
}

#[test]
fn boundary_condition_reference() {
    assert_reference_retirement(
        r###"function keep(&$v){return true;}$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];if(keep($tree['branch'])){unset($tree);}echo gc_collect_cycles(),'|';"###,
        "1|",
    );
}

#[test]
fn boundary_echo_reference() {
    assert_reference_retirement(
        r###"function keep(&$v){return 'value|';}$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];echo keep($tree['branch']);unset($tree);echo gc_collect_cycles(),'|';"###,
        "value|1|",
    );
}

#[test]
fn boundary_multi_cv_user_between() {
    assert_reference_retirement(
        r###"function keep(&$a,$x,&$b){echo $x,'|';}$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];$second=['branch'=>[]];$second['branch']['loop'] =& $second['branch'];keep($tree['branch'],$user='live',$second['branch']);unset($tree,$second);echo $user,'|';echo gc_collect_cycles(),'|';"###,
        "live|live|2|",
    );
}

#[test]
fn boundary_exception_after_send() {
    assert_reference_retirement(
        r###"function keep(&$a,$b){}function fail(){throw new Exception('fail');}$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];try{keep($tree['branch'],fail());}catch(Exception $e){unset($tree);echo gc_collect_cycles(),'|';}"###,
        "1|",
    );
}

#[test]
fn boundary_exception_finally() {
    assert_reference_retirement(
        r###"function keep(&$a,$b){}function fail(){throw new Exception('fail');}$tree=['branch'=>['value'=>19]];$tree['branch']['loop'] =& $tree['branch'];try{try{keep($tree['branch'],fail());}finally{unset($tree);echo gc_collect_cycles(),'|';}}catch(Exception $e){echo gc_collect_cycles(),'|';}"###,
        "1|0|",
    );
}

fn assert_reference_retirement(source: &str, expected: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-dzend.enable_gc=1",
            "-r",
            source,
        ])
        .output()
        .expect("run reference retirement probe");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn statement_end() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];echo gc_collect_cycles(), '|';unset($tree);echo gc_collect_cycles(), '|';"###,
        "0|1|",
    );
}

#[test]
fn replace_root() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];$tree = 'released';echo gc_collect_cycles(), '|';echo $tree;"###,
        "1|released",
    );
}

#[test]
fn rebind_root() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];$replacement = 'safe'; $tree =& $replacement;echo gc_collect_cycles(), '|';echo $replacement;"###,
        "1|safe",
    );
}

#[test]
fn live_reference() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];$keep =& $tree['branch']; unset($tree);echo gc_collect_cycles(), '|';unset($keep);echo gc_collect_cycles(), '|';"###,
        "0|1|",
    );
}

#[test]
fn cow_container() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];$copy = $tree; unset($tree);echo gc_collect_cycles(), '|';unset($copy);echo gc_collect_cycles(), '|';"###,
        "0|1|",
    );
}

#[test]
fn function_completion() {
    assert_reference_retirement(
        r###"function buildTree() {$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];} buildTree();echo gc_collect_cycles(), '|';"###,
        "1|",
    );
}

#[test]
fn function_returned_owner() {
    assert_reference_retirement(
        r###"function buildTree() {$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];return $tree;} $result=buildTree();echo gc_collect_cycles(), '|';unset($result);echo gc_collect_cycles(), '|';"###,
        "0|1|",
    );
}

#[test]
fn reference_parameter() {
    assert_reference_retirement(
        r###"function makeLoop(&$item) { $item['loop'] =& $item; } $tree=['branch'=>['value'=>19]]; makeLoop($tree['branch']);echo gc_collect_cycles(), '|';unset($tree);echo gc_collect_cycles(), '|';"###,
        "0|1|",
    );
}

#[test]
fn array_reference_literal() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];$aliases = [&$tree['branch']]; unset($tree);echo gc_collect_cycles(), '|';unset($aliases);echo gc_collect_cycles(), '|';"###,
        "0|1|",
    );
}

#[test]
fn reference_destructure() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];[$alias] = [&$tree['branch']]; unset($tree);echo gc_collect_cycles(), '|';unset($alias);echo gc_collect_cycles(), '|';"###,
        "0|1|",
    );
}

#[test]
fn property_source() {
    assert_reference_retirement(
        r###"$owner=new stdClass; $owner->nested=['value'=>19]; $owner->nested['loop'] =& $owner->nested;echo gc_collect_cycles(), '|';unset($owner);echo gc_collect_cycles(), '|';"###,
        "0|1|",
    );
}

#[test]
fn nested_root_and_child() {
    assert_reference_retirement(
        r###"$tree=[]; $tree['root'] =& $tree; $tree['branch']=['value'=>19]; $tree['branch']['loop'] =& $tree['branch']; $tree = 'new';echo gc_collect_cycles(), '|';"###,
        "1|",
    );
}

#[test]
fn ref_foreach_completion() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];foreach($tree as &$item){echo $item['value'],'|';break;}unset($tree);echo gc_collect_cycles(), '|';unset($item);echo gc_collect_cycles(), '|';"###,
        "19|0|1|",
    );
}

#[test]
fn many_small_cycles() {
    assert_reference_retirement(
        r###"for($i=0;$i<37;++$i){$tree=['branch'=>['value'=>$i]];$tree['branch']['loop'] =& $tree['branch'];unset($tree);}echo gc_collect_cycles(), '|';"###,
        "37|",
    );
}

#[test]
fn byref_call_live_argument() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];function inspect(&$part,$during){echo $part['value'],':',$during,'|';} inspect($tree['branch'],gc_collect_cycles());unset($tree);echo gc_collect_cycles(), '|';"###,
        "19:0|1|",
    );
}

#[test]
fn reentrant_argument_removal() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];function releaseRoot(){unset($GLOBALS['tree']);return gc_collect_cycles();}function inspect(&$part,$during){echo $part['value'],':',$during,'|';}inspect($tree['branch'],releaseRoot());echo gc_collect_cycles(), '|';"###,
        "19:0|1|",
    );
}

#[test]
fn suspended_fiber_argument() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];function inspect(&$part,$during){echo $part['value'],':',$during,'|';}$fiber=new Fiber(function()use(&$tree){inspect($tree['branch'],Fiber::suspend('parked'));});echo $fiber->start(),'|';unset($tree);echo gc_collect_cycles(), '|';$fiber->resume(23);unset($fiber);echo gc_collect_cycles(), '|';"###,
        "parked|0|19:23|1|",
    );
}

#[test]
fn suspended_generator_argument() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];function inspect(&$part,$during){echo $part['value'],':',$during,'|';}function parked(&$tree){inspect($tree['branch'],yield 'parked');}$generator=parked($tree);echo $generator->current(),'|';unset($tree);echo gc_collect_cycles(), '|';$generator->send(23);unset($generator);echo gc_collect_cycles(), '|';"###,
        "parked|0|19:23|1|",
    );
}

#[test]
fn failed_reference_target() {
    assert_reference_retirement(
        r###"$tree = ['branch' => ['value' => 19]]; $tree['branch']['loop'] =& $tree['branch'];function rejectKey(){throw new Exception('blocked');}try{$target[rejectKey()] =& $tree['branch'];}catch(Exception $e){echo $e->getMessage(),'|';}unset($tree);echo gc_collect_cycles(), '|';"###,
        "blocked|1|",
    );
}

#[test]
fn destructor_witness() {
    assert_reference_retirement(
        r###"class Witness{function __destruct(){echo 'drop|';}}$tree=['branch'=>['witness'=>new Witness]];$tree['branch']['loop'] =& $tree['branch'];echo 'before|';unset($tree);echo gc_collect_cycles(), '|';echo 'after|';"###,
        "before|drop|1|after|",
    );
}
