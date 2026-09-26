use std::process::Command;

#[test]
fn constant_scalar_reference_snapshot() {
    assert_snapshot_contract(
        r###"$v=13;$input=['nested'=>['value'=>&$v]];define('SAMPLE_SNAPSHOT',$input);$v=29;echo SAMPLE_SNAPSHOT['nested']['value'],':',$input['nested']['value'],'|';"###,
        "13:29|",
    );
}

#[test]
fn constant_shared_array_is_not_recursion() {
    assert_snapshot_contract(
        r###"$leaf=['value'=>11];$input=['left'=>&$leaf,'right'=>&$leaf];define('SAMPLE_SNAPSHOT',$input);$leaf['value']=31;echo SAMPLE_SNAPSHOT['left']['value'],':',SAMPLE_SNAPSHOT['right']['value'],'|';"###,
        "11:11|",
    );
}

#[test]
fn constant_object_identity_is_preserved() {
    assert_snapshot_contract(
        r###"$object=(object)['value'=>11];$input=['object'=>&$object];define('SAMPLE_SNAPSHOT',$input);$saved=$object;$object=(object)['value'=>31];$saved->value=19;echo SAMPLE_SNAPSHOT['object']->value,':',(int)(SAMPLE_SNAPSHOT['object']===$saved),'|';"###,
        "19:1|",
    );
}

#[test]
fn constant_object_backedge_is_not_array_recursion() {
    assert_snapshot_contract(
        r###"$object=new stdClass;$input=[$object];$object->back=&$input;define('SAMPLE_SNAPSHOT',$input);echo (int)(SAMPLE_SNAPSHOT[0]===$object),'|';unset($input);"###,
        "1|",
    );
}

#[test]
fn constant_rejects_direct_array_cycle() {
    assert_snapshot_contract(
        r###"$input=['value'=>8];$input['loop']=&$input;try{define('SAMPLE_SNAPSHOT',$input);}catch(ValueError $e){echo $e->getMessage(),'|';}echo (int)defined('SAMPLE_SNAPSHOT'),':',$input['value'],'|';"###,
        "define(): Argument #2 ($value) cannot be a recursive array|0:8|",
    );
}

#[test]
fn constant_rejects_indirect_array_cycle() {
    assert_snapshot_contract(
        r###"$first=[];$second=['back'=>&$first];$first['forward']=&$second;try{define('SAMPLE_SNAPSHOT',$first);}catch(ValueError $e){echo $e->getMessage(),'|';}echo (int)defined('SAMPLE_SNAPSHOT'),'|';"###,
        "define(): Argument #2 ($value) cannot be a recursive array|0|",
    );
}

#[test]
fn constant_duplicate_and_recursive_priority() {
    assert_snapshot_contract(
        r###"define('SAMPLE_SNAPSHOT',19);$input=[];$input['loop']=&$input;set_error_handler(function($level,$text){echo $text,'|';return true;});try{var_dump(define('SAMPLE_SNAPSHOT',$input));}catch(ValueError $e){echo $e->getMessage(),'|';}echo SAMPLE_SNAPSHOT,'|';"###,
        "define(): Argument #2 ($value) cannot be a recursive array|19|",
    );
}

#[test]
fn constant_invalid_name_precedes_recursive_value() {
    assert_snapshot_contract(
        r###"$input=[];$input['loop']=&$input;try{define('Owner::FIELD',$input);}catch(ValueError $e){echo $e->getMessage(),'|';}"###,
        "define(): Argument #1 ($constant_name) cannot be a class constant|",
    );
}

#[test]
fn constant_flag_warning_precedes_recursive_value() {
    assert_snapshot_contract(
        r###"$input=[];$input['loop']=&$input;set_error_handler(function($level,$text){echo $text,'|';return true;});try{define('SAMPLE_SNAPSHOT',$input,true);}catch(ValueError $e){echo $e->getMessage(),'|';}"###,
        "define(): Argument #3 ($case_insensitive) is ignored since declaration of case-insensitive constants is no longer supported|define(): Argument #2 ($value) cannot be a recursive array|",
    );
}

#[test]
fn foreach_self_alias_target() {
    assert_snapshot_contract(
        r###"$field='branch';foreach(($node=[$field=>[$field=>&$node]]) as $node){echo (int)($node[$field]===$node),'|';var_dump($node);}"###,
        "1|array(1) {\n  [\"branch\"]=>\n  *RECURSION*\n}\n",
    );
}

#[test]
fn foreach_self_alias_through_live_cell() {
    assert_snapshot_contract(
        r###"$alias=&$node;foreach(($node=['first'=>['inside'=>&$node]]) as $node){echo (int)($node['inside']===$node),':',(int)($alias===$node),'|';}echo (int)($alias===$node),'|';"###,
        "1:1|1|",
    );
}

#[test]
fn foreach_distinct_destination_keeps_outer_source() {
    assert_snapshot_contract(
        r###"$node=['item'=>['inside'=>&$node]];foreach($node as $value){echo (int)($value['inside']===$node),'|';}echo count($node),'|';"###,
        "1|1|",
    );
}

#[test]
fn foreach_self_target_plain_array() {
    assert_snapshot_contract(
        r###"$node=[[13],[29]];foreach($node as $node){echo $node[0],'|';}echo $node[0],'|';"###,
        "13|29|29|",
    );
}

#[test]
fn foreach_literal_reference_target() {
    assert_snapshot_contract(
        r###"$slot=5;$aliases=[&$slot];foreach([17,23] as $slot){echo $aliases[0],'|';}echo $slot,'|';"###,
        "17|23|23|",
    );
}

#[test]
fn foreach_nested_literal_reference_target() {
    assert_snapshot_contract(
        r###"function sample(){$slot=5;$aliases=['nested'=>[&$slot]];foreach([17,23] as $slot){echo $aliases['nested'][0],'|';}}sample();"###,
        "17|23|",
    );
}

#[test]
fn foreach_reference_literal_after_loop() {
    assert_snapshot_contract(
        r###"$slot=5;foreach([17,23] as $slot){echo $slot,'|';}$aliases=[&$slot];$slot=31;echo $aliases[0],'|';"###,
        "17|23|31|",
    );
}

#[test]
fn foreach_by_value_literal_does_not_alias() {
    assert_snapshot_contract(
        r###"$slot=5;$copy=[$slot];foreach([17,23] as $slot){echo $copy[0],':',$slot,'|';}"###,
        "5:17|5:23|",
    );
}

#[test]
fn foreach_literal_reference_key_target() {
    assert_snapshot_contract(
        r###"$key=5;$aliases=[&$key];foreach(['first'=>17,'second'=>23] as $key=>$value){echo $aliases[0],':',$value,'|';}"###,
        "first:17|second:23|",
    );
}

#[test]
fn constant_returned_array_stays_independent() {
    assert_snapshot_contract(
        r###"$cell=13;$input=[&$cell];define('SAMPLE_SNAPSHOT',$input);$copy=SAMPLE_SNAPSHOT;$copy[0]=29;echo $cell,':',SAMPLE_SNAPSHOT[0],':',$copy[0],'|';"###,
        "13:13:29|",
    );
}

#[test]
fn constant_preserves_sparse_keys_and_append_index() {
    assert_snapshot_contract(
        r###"$input=[-4=>'a',9=>'b','tag'=>'c'];unset($input[9]);next($input);define('SAMPLE_SNAPSHOT',$input);$copy=SAMPLE_SNAPSHOT;echo key($copy),'|';$copy[]='d';foreach($copy as $key=>$value){echo $key,':',$value,'|';}"###,
        "-4|-4:a|tag:c|-3:d|",
    );
}

#[test]
fn constant_binary_keys_and_values() {
    assert_snapshot_contract(
        r###"$cell="\x80\xff";$key="\xff";$input=[$key=>&$cell];define('SAMPLE_SNAPSHOT',$input);$cell='later';foreach(SAMPLE_SNAPSHOT as $key=>$value){echo bin2hex($key),':',bin2hex($value),'|';}"###,
        "ff:80ff|",
    );
}

#[test]
fn constant_closure_identity() {
    assert_snapshot_contract(
        r###"$cell=13;$callback=function()use(&$cell){return $cell;};$input=[&$callback];define('SAMPLE_SNAPSHOT',$input);$original=$callback;$callback=function(){return 99;};$cell=29;echo SAMPLE_SNAPSHOT[0](),':',(int)(SAMPLE_SNAPSHOT[0]===$original),'|';"###,
        "29:1|",
    );
}

#[test]
fn constant_resource_identity() {
    assert_snapshot_contract(
        r###"$stream=fopen('php://memory','w+');define('SAMPLE_SNAPSHOT',[&$stream]);$saved=$stream;$stream=null;fwrite(SAMPLE_SNAPSHOT[0],'ok');rewind($saved);echo fread($saved,2),'|';fclose($saved);"###,
        "ok|",
    );
}

#[test]
fn constant_input_aliases_survive_failed_cycle() {
    assert_snapshot_contract(
        r###"$input=[];$alias=&$input;$input['back']=&$input;try{define('SAMPLE_SNAPSHOT',$input);}catch(ValueError $e){echo 'rejected|';}unset($input['back']);$alias['value']=29;echo $input['value'],':',(int)defined('SAMPLE_SNAPSHOT'),'|';"###,
        "rejected|29:0|",
    );
}

#[test]
fn constant_duplicate_nonrecursive_warns_after_projection() {
    assert_snapshot_contract(
        r###"define('SAMPLE_SNAPSHOT',13);$cell=29;$input=[&$cell];set_error_handler(function($level,$text){echo $text,'|';return true;});var_dump(define('SAMPLE_SNAPSHOT',$input));echo $cell,':',SAMPLE_SNAPSHOT,'|';"###,
        "Constant SAMPLE_SNAPSHOT already defined, this will be an error in PHP 9|bool(false)\n29:13|",
    );
}

#[test]
fn constant_flag_exception_preserves_input() {
    assert_snapshot_contract(
        r###"$cell=13;$input=[&$cell];set_error_handler(function(){throw new Exception('warning');});try{define('SAMPLE_SNAPSHOT',$input,true);}catch(Exception $e){echo $e->getMessage(),'|';}$cell=29;echo $input[0],':',(int)defined('SAMPLE_SNAPSHOT'),'|';"###,
        "warning|29:1|",
    );
}

#[test]
fn constant_flag_exception_precedes_cycle_error() {
    assert_snapshot_contract(
        r###"$input=[];$input['back']=&$input;set_error_handler(function(){throw new Exception('warning');});try{define('SAMPLE_SNAPSHOT',$input,true);}catch(Throwable $e){for(;$e;$e=$e->getPrevious())echo get_class($e),':',$e->getMessage(),'|';}echo (int)defined('SAMPLE_SNAPSHOT'),'|';"###,
        "Exception:warning|0|",
    );
}

#[test]
fn constant_deep_acyclic_snapshot() {
    assert_snapshot_contract(
        r###"$cell=13;$input=['value'=>&$cell];for($i=0;$i<300;$i++)$input=['child'=>$input];define('SAMPLE_SNAPSHOT',$input);$cell=29;$copy=SAMPLE_SNAPSHOT;for($i=0;$i<300;$i++)$copy=$copy['child'];echo $copy['value'],'|';"###,
        "13|",
    );
}

#[test]
fn constant_unchanged_scalar_and_object() {
    assert_snapshot_contract(
        r###"$value=13;define('SAMPLE_SNAPSHOT',$value);$value=29;echo SAMPLE_SNAPSHOT,'|';$object=(object)['value'=>17];define('SAMPLE_OBJECT',$object);$object->value=31;echo SAMPLE_OBJECT->value,':',(int)(SAMPLE_OBJECT===$object),'|';"###,
        "13|31:1|",
    );
}

fn assert_snapshot_contract(source: &str, expected: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0", "-r", source])
        .output()
        .expect("run core array snapshot contract");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}
