mod common;

#[test]
fn callback_replenishment_does_not_readmit_the_triggering_owner() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Renewal {static $allow=true;static $created=0;static $released=0;public $self;
function __construct(){$this->self=$this;++self::$created;}
function __destruct(){++self::$released;if(self::$allow&&self::$created<40010)new Renewal;}}
register_shutdown_function(function(){Renewal::$allow=false;});
for($i=0;$i<10001;$i++)new Renewal;
Renewal::$allow=false;$s=gc_status();
echo Renewal::$created,':',Renewal::$released,':',$s['runs'],':',$s['roots'],':',$s['threshold'],'|';
"#
        ),
        "30001:20000:2:20001:20001|"
    );
}

#[test]
fn a_new_release_after_callback_replenishment_still_admits_collection() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Renewal {static $allow=true;static $created=0;static $released=0;public $self;
function __construct(){$this->self=$this;++self::$created;}
function __destruct(){++self::$released;if(self::$allow&&self::$created<40010)new Renewal;}}
register_shutdown_function(function(){Renewal::$allow=false;});
for($i=0;$i<10002;$i++)new Renewal;
Renewal::$allow=false;$s=gc_status();
echo Renewal::$created,':',Renewal::$released,':',$s['runs'],':',$s['roots'],':',$s['threshold'],'|';
"#
        ),
        "40010:40002:4:10009:10001|"
    );
}

#[test]
fn unsetting_a_cyclic_reference_leaves_child_destructors_for_collection() {
    assert_eq!(
        common::run_php(
            r#"<?php
gc_disable();class Child {function __destruct(){echo 'drop|';}}
$cycle=[new Child];$cycle[]=&$cycle;unset($cycle);echo 'unset|';
gc_collect_cycles();echo 'collected|';
"#
        ),
        "unset|drop|collected|"
    );
}

#[test]
fn shared_reference_unset_preserves_objects_and_nested_arrays_until_final_owner() {
    assert_eq!(
        common::run_php(
            r#"<?php
class Child {function __construct(public $name){}function __destruct(){echo $this->name,'|';}}
$first=new Child('object');$alias=&$first;unset($first);echo 'one|';unset($alias);
$first=[new Child('array')];$alias=&$first;unset($first);echo 'two|';unset($alias);
$first=[new Child('write')];$alias=&$first;$first=[];echo count($alias),'|';
"#
        ),
        "one|object|two|array|write|0|"
    );
}

#[test]
fn reference_promotion_updates_scope_mirrors_without_admitting_the_same_value() {
    let source = r#"gc_disable();function inspect_roots(){echo gc_status()['roots'],'|';}
$cycle=[new stdClass];inspect_roots();$cycle[]=&$cycle;inspect_roots();
unset($cycle);inspect_roots();"#;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0", "-r", source])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0|0|1|");
    assert!(output.stderr.is_empty());
}

#[test]
fn repeated_call_scope_mirrors_are_not_php_self_assignments() {
    assert_eq!(
        common::run_php(
            r#"<?php
gc_disable();function report_roots(){$s=gc_status();echo $s['roots'],'|';}
class Owner {public $child;public $parent;}
$root=new Owner;report_roots();$root->child=new Owner;report_roots();
$root->child->parent=$root;report_roots();
$root=$root;report_roots();unset($root);echo gc_collect_cycles(),'|';
"#
        ),
        "0|0|0|1|2|"
    );
}

#[test]
fn property_commit_and_receiver_reads_do_not_publish_temporary_roots() {
    assert_eq!(
        common::run_php(
            r#"<?php
gc_disable(); class Owner {public $child; public $parent;}
$root=new Owner; echo gc_status()['roots'],'|';
$root->child=new Owner; echo gc_status()['roots'],'|';
$root->child->parent=$root; echo gc_status()['roots'],'|';
unset($root); echo gc_collect_cycles(),'|';
"#
        ),
        "0|0|0|2|"
    );
}

#[test]
fn property_statement_preserves_real_source_alias_admission() {
    assert_eq!(
        common::run_php(
            r#"<?php
gc_disable(); class Owner {public $child;}
$root=new Owner;$child=new Owner;$root->child=$child;
echo gc_status()['roots'],'|';unset($child);echo gc_status()['roots'],'|';
"#
        ),
        "0|1|"
    );
}

#[test]
fn cold_and_warm_property_storage_transfers_have_the_same_gc_roots() {
    assert_eq!(
        common::run_php(
            r#"<?php
gc_disable(); class Owner {public $child;}
$root=new Owner;for($i=0;$i<3;$i++){$root->child=new Owner;echo gc_status()['roots'],'|';}
"#
        ),
        "0|0|0|"
    );
}

#[test]
fn property_assignment_result_becomes_a_real_php_owner() {
    assert_eq!(
        common::run_php(
            r#"<?php
gc_disable();class Owner {public $child;}$root=new Owner;
$copy=($root->child=new Owner);echo gc_status()['roots'],'|';
unset($copy);echo gc_status()['roots'],'|';
"#
        ),
        "0|1|"
    );
}

#[test]
fn virtual_property_result_still_publishes_its_unretained_cycle() {
    assert_eq!(
        common::run_php(
            r#"<?php
gc_disable();class Node {public $child;public $self;function __construct(){$this->self=$this;}}
class Owner {function __get($name){return new Node;}}
$root=new Owner;gc_collect_cycles();$root->child->child=7;
echo gc_status()['roots'],'|',gc_collect_cycles(),'|';
"#
        ),
        "2|1|"
    );
}

#[test]
fn admission_array_4() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
for($i=0;$i<4;$i++){ $cycle=[];$cycle[]=&$cycle;unset($cycle); } $s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],'|',gc_collect_cycles(),'|';
"#
        ),
        "0:4:0|4|"
    );
}

#[test]
fn admission_array_9999() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
for($i=0;$i<9999;$i++){ $cycle=[];$cycle[]=&$cycle;unset($cycle); } $s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],'|',gc_collect_cycles(),'|';
"#
        ),
        "0:9999:0|9999|"
    );
}

#[test]
fn admission_array_10000() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
for($i=0;$i<10000;$i++){ $cycle=[];$cycle[]=&$cycle;unset($cycle); } $s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],'|',gc_collect_cycles(),'|';
"#
        ),
        "0:10000:0|10000|"
    );
}

#[test]
fn admission_array_10001() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
for($i=0;$i<10001;$i++){ $cycle=[];$cycle[]=&$cycle;unset($cycle); } $s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],'|',gc_collect_cycles(),'|';
"#
        ),
        "1:1:10000|1|"
    );
}

#[test]
fn admission_array_10002() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
for($i=0;$i<10002;$i++){ $cycle=[];$cycle[]=&$cycle;unset($cycle); } $s=gc_status();echo $s['runs'],':',$s['roots'],':',$s['collected'],'|',gc_collect_cycles(),'|';
"#
        ),
        "1:2:10000|2|"
    );
}

#[test]
fn admission_disabled() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
gc_disable();for($i=0;$i<10002;$i++){ $cycle=[];$cycle[]=&$cycle;unset($cycle); }$s=gc_status();echo $s["runs"],":",$s["roots"],"|";gc_enable();$s=gc_status();echo $s["runs"],":",$s["roots"],"|"; $cycle=[];$cycle[]=&$cycle;unset($cycle);$s=gc_status();echo $s["runs"],":",$s["roots"],"|",gc_collect_cycles();
"#
        ),
        "0:10002|0:10002|1:1|1"
    );
}

#[test]
fn admission_object() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class AdmissionNode { public $self; public function __construct(){ $this->self=$this; } } for($i=0;$i<10001;$i++){new AdmissionNode;} $s=gc_status();echo $s["runs"],":",$s["roots"],":",$s["collected"],"|",gc_collect_cycles();
"#
        ),
        "1:1:10000|1"
    );
}

#[test]
fn admission_retained() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class RetainedAdmission { public $self; function __construct(){ $this->self=$this; } } $keep=[];for($i=0;$i<10001;$i++){ $v=new RetainedAdmission;$keep[]=$v;unset($v); }$s=gc_status();echo $s["runs"],":",$s["roots"],":",$s["threshold"],":",$s["collected"],"|";unset($keep);echo gc_collect_cycles(),"|";$s=gc_status();echo $s["runs"],":",$s["roots"],":",$s["threshold"],":",$s["collected"];
"#
        ),
        "1:1:20001:0|10001|2:0:20001:10001"
    );
}

#[test]
fn boundary_dead_owners() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
for($i=0;$i<10005;$i++){ $v=new stdClass; $alias=$v;unset($v,$alias); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:0:0:10001:0|"
    );
}

#[test]
fn boundary_adaptive_0() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ProbeCycle {public $self;function __construct(){$this->self=$this;}}
  $keep=[];for($i=0;$i<10001;$i++){ $v=new ProbeCycle;if($i>=0)$keep[]=$v;unset($v); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:1:1:20001:0|"
    );
}

#[test]
fn boundary_adaptive_50() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ProbeCycle {public $self;function __construct(){$this->self=$this;}}
  $keep=[];for($i=0;$i<10001;$i++){ $v=new ProbeCycle;if($i>=50)$keep[]=$v;unset($v); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:1:1:20001:50|"
    );
}

#[test]
fn boundary_adaptive_99() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ProbeCycle {public $self;function __construct(){$this->self=$this;}}
  $keep=[];for($i=0;$i<10001;$i++){ $v=new ProbeCycle;if($i>=99)$keep[]=$v;unset($v); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:1:1:20001:99|"
    );
}

#[test]
fn boundary_adaptive_100() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ProbeCycle {public $self;function __construct(){$this->self=$this;}}
  $keep=[];for($i=0;$i<10001;$i++){ $v=new ProbeCycle;if($i>=100)$keep[]=$v;unset($v); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:1:1:10001:100|"
    );
}

#[test]
fn boundary_adaptive_101() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ProbeCycle {public $self;function __construct(){$this->self=$this;}}
  $keep=[];for($i=0;$i<10001;$i++){ $v=new ProbeCycle;if($i>=101)$keep[]=$v;unset($v); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:1:1:10001:101|"
    );
}

#[test]
fn boundary_adaptive_1000() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ProbeCycle {public $self;function __construct(){$this->self=$this;}}
  $keep=[];for($i=0;$i<10001;$i++){ $v=new ProbeCycle;if($i>=1000)$keep[]=$v;unset($v); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:1:1:10001:1000|"
    );
}

#[test]
fn boundary_adaptive_second_useful_pass() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ProbeCycle {public $self;function __construct(){$this->self=$this;}}
$keep=[];for($i=0;$i<10001;$i++){ $v=new ProbeCycle;$keep[]=$v;unset($v); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
unset($keep);for($i=0;$i<20001;$i++){ $v=[];$v[]=&$v;unset($v); }$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';echo gc_collect_cycles(),'|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:1:1:20001:0|0:3:2:10001:30000|2|0:4:0:10001:30002|"
    );
}

#[test]
fn boundary_disabled_explicit() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
gc_disable();for($i=0;$i<10001;$i++){$v=[];$v[]=&$v;unset($v);}echo gc_collect_cycles(),'|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';gc_enable();$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "10001|0:1:0:10001:10001|0:1:0:10001:10001|"
    );
}

#[test]
fn boundary_callback_status() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class StatusCycle {public $self;function __construct(){$this->self=$this;}
function __destruct(){echo 'drop|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';echo 'recursive:',gc_collect_cycles(),'|';}}
$v=new StatusCycle;unset($v);for($i=0;$i<9999;$i++){$v=[];$v[]=&$v;unset($v);}
echo 'before|';$v=[];$v[]=&$v;unset($v);echo 'after|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';echo gc_collect_cycles(),'|';
"#
        ),
        "before|drop|1:1:10000:10001:0|recursive:0|after|0:2:1:10001:10000|1|"
    );
}

#[test]
fn boundary_callback_exception() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ThrowCycle {public $self;function __construct(){$this->self=$this;}
function __destruct(){echo 'drop|';throw new LogicException('gc');}}
$v=new ThrowCycle;unset($v);for($i=0;$i<9999;$i++){$v=[];$v[]=&$v;unset($v);}
try {$trigger=[];$trigger[]=&$trigger;unset($trigger);echo 'unexpected|';}
catch(Throwable $error){echo get_class($error),':',$error->getMessage(),':',(int)isset($trigger),'|';}
$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';echo gc_collect_cycles(),'|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "drop|LogicException:gc:0|0:2:2:10001:10000|1|0:3:0:10001:10001|"
    );
}

#[test]
fn boundary_callback_resurrection() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class SavedCycle {public $self;function __construct(){$this->self=$this;}
function __destruct(){echo 'drop|';$GLOBALS['saved']=$this;}}
$v=new SavedCycle;unset($v);for($i=0;$i<9999;$i++){$v=[];$v[]=&$v;unset($v);}
$v=[];$v[]=&$v;unset($v);echo (int)isset($saved),'|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';unset($saved);echo gc_collect_cycles(),'|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "drop|1|0:2:1:10001:9999|2|0:3:0:10001:10001|"
    );
}

#[test]
fn boundary_callback_creates_cycle() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class NewCycle {public $self;function __construct(){$this->self=$this;}
function __destruct(){echo 'drop|';$fresh=[];$fresh[]=&$fresh;unset($fresh);}}
$v=new NewCycle;unset($v);for($i=0;$i<9999;$i++){$v=[];$v[]=&$v;unset($v);}
$v=[];$v[]=&$v;unset($v);$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';echo gc_collect_cycles(),'|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "drop|0:2:1:10001:10001|1|0:3:0:10001:10002|"
    );
}

#[test]
fn boundary_repeated_admission() {
    assert_eq!(
        common::run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
for($i=0;$i<30003;$i++){$v=[];$v[]=&$v;unset($v);}$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';echo gc_collect_cycles(),'|';$s=gc_status();echo (int)$s['running'],':',$s['runs'],':',$s['roots'],':',$s['threshold'],':',$s['collected'],'|';
"#
        ),
        "0:3:3:10001:30000|3|0:4:0:10001:30003|"
    );
}
