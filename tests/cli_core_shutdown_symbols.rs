use std::process::Command;

#[test]
fn error_string() {
    assert_shutdown_contract(
        r###"$saved='visible';function source(){try{yield 3;}finally{echo $GLOBALS['saved'],'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|visible|",
    );
}

#[test]
fn exception_string() {
    assert_shutdown_contract(
        r###"$saved='visible';function source(){try{yield 3;}finally{echo $GLOBALS['saved'],'|';}}$it=source();$it->current();set_exception_handler(function($e)use($it){});echo 'end|';"###,
        "end|visible|",
    );
}

#[test]
fn direct_string() {
    assert_shutdown_contract(
        r###"$saved='visible';function source(){try{yield 3;}finally{echo $GLOBALS['saved'],'|';}}$it=source();$it->current();echo 'end|';"###,
        "end|visible|",
    );
}

#[test]
fn error_integer() {
    assert_shutdown_contract(
        r###"$saved=17;function source(){try{yield 3;}finally{echo $GLOBALS['saved'],'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|17|",
    );
}

#[test]
fn exception_integer() {
    assert_shutdown_contract(
        r###"$saved=17;function source(){try{yield 3;}finally{echo $GLOBALS['saved'],'|';}}$it=source();$it->current();set_exception_handler(function($e)use($it){});echo 'end|';"###,
        "end|17|",
    );
}

#[test]
fn direct_integer() {
    assert_shutdown_contract(
        r###"$saved=17;function source(){try{yield 3;}finally{echo $GLOBALS['saved'],'|';}}$it=source();$it->current();echo 'end|';"###,
        "end|17|",
    );
}

#[test]
fn error_array() {
    assert_shutdown_contract(
        r###"$saved=['value'=>19];function source(){try{yield 3;}finally{echo $GLOBALS['saved']['value'],'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|19|",
    );
}

#[test]
fn exception_array() {
    assert_shutdown_contract(
        r###"$saved=['value'=>19];function source(){try{yield 3;}finally{echo $GLOBALS['saved']['value'],'|';}}$it=source();$it->current();set_exception_handler(function($e)use($it){});echo 'end|';"###,
        "end|19|",
    );
}

#[test]
fn direct_array() {
    assert_shutdown_contract(
        r###"$saved=['value'=>19];function source(){try{yield 3;}finally{echo $GLOBALS['saved']['value'],'|';}}$it=source();$it->current();echo 'end|';"###,
        "end|19|",
    );
}

#[test]
fn error_object() {
    assert_shutdown_contract(
        r###"$saved=(object)['value'=>23];function source(){try{yield 3;}finally{echo $GLOBALS['saved']->value,'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end||",
    );
}

#[test]
fn exception_object() {
    assert_shutdown_contract(
        r###"$saved=(object)['value'=>23];function source(){try{yield 3;}finally{echo $GLOBALS['saved']->value,'|';}}$it=source();$it->current();set_exception_handler(function($e)use($it){});echo 'end|';"###,
        "end|\nWarning: Undefined global variable $saved in Command line code on line 1\n\nWarning: Attempt to read property \"value\" on null in Command line code on line 1\n|",
    );
}

#[test]
fn direct_object() {
    assert_shutdown_contract(
        r###"$saved=(object)['value'=>23];function source(){try{yield 3;}finally{echo $GLOBALS['saved']->value,'|';}}$it=source();$it->current();echo 'end|';"###,
        "end|23|",
    );
}

#[test]
fn handler_global_cell() {
    assert_shutdown_contract(
        r###"$saved='visible';$alias=&$saved;function source(){try{yield 3;}finally{global $saved;echo $saved,'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|visible|",
    );
}

#[test]
fn handler_missing_symbol() {
    assert_shutdown_contract(
        r###"function source(){try{yield 3;}finally{echo (int)isset($GLOBALS['missing']),'|';}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|0|",
    );
}

#[test]
fn handler_shutdown_mutation() {
    assert_shutdown_contract(
        r###"$saved='before';function source(){try{yield 3;}finally{echo $GLOBALS['saved'],'|';}}$it=source();$it->current();set_error_handler(function()use($it){});register_shutdown_function(function(){$GLOBALS['saved']='after';echo 'shutdown|';});echo 'end|';"###,
        "end|shutdown|after|",
    );
}

#[test]
fn array_object() {
    assert_shutdown_contract(
        r###"class Sample{public $value=29;function __destruct(){echo 'drop|';}}$saved=[new Sample];function inspect(){echo $GLOBALS['saved'][0]->value,'|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|drop|29|",
    );
}

#[test]
fn array_closure() {
    assert_shutdown_contract(
        r###"$saved=[fn()=>29];function inspect(){echo $GLOBALS['saved'][0](),'|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|29|",
    );
}

#[test]
fn global_closure() {
    assert_shutdown_contract(
        r###"$saved=fn()=>29;function inspect(){echo isset($GLOBALS['saved'])?'present|':'absent|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|absent|",
    );
}

#[test]
fn global_resource() {
    assert_shutdown_contract(
        r###"$saved=fopen('php://memory','w+');fwrite($saved,'abc');rewind($saved);function inspect(){echo fread($GLOBALS['saved'],3),'|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|abc|",
    );
}

#[test]
fn reference_object() {
    assert_shutdown_contract(
        r###"$saved=(object)['value'=>29];$alias=&$saved;function inspect(){echo isset($GLOBALS['saved'])?'present|':'absent|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|present|",
    );
}

#[test]
fn reference_array() {
    assert_shutdown_contract(
        r###"$saved=['value'=>29];$alias=&$saved;function inspect(){global $saved;echo $saved['value'],'|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|29|",
    );
}

#[test]
fn callback_read_write() {
    assert_shutdown_contract(
        r###"$saved='before';function inspect(){global $saved;echo $saved,'|';$saved='after';echo $GLOBALS['saved'],'|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|before|after|",
    );
}

#[test]
fn handler_survives_warning() {
    assert_shutdown_contract(
        r###"function source(){try{yield 3;}finally{trigger_error('late',E_USER_WARNING);echo 'finally|';}}$it=source();$it->current();set_error_handler(function($level,$text)use($it){echo $text,'|';return true;});echo 'end|';"###,
        "end|late|finally|",
    );
}

#[test]
fn handler_retirement_stack() {
    assert_shutdown_contract(
        r###"$saved='visible';function source($name){try{yield 3;}finally{echo $name,':',$GLOBALS['saved'],'|';}}$first=source('first');$first->current();set_error_handler(function()use($first){});$second=source('second');$second->current();set_error_handler(function()use($second){});echo 'end|';"###,
        "end|first:visible|second:visible|",
    );
}

#[test]
fn exception_handler_reads_global() {
    assert_shutdown_contract(
        r###"$saved='visible';class Sample{function __destruct(){throw new Exception('late');}}$owner=new Sample;set_exception_handler(function($e){echo $GLOBALS['saved'],':',$e->getMessage(),'|';});echo 'end|';"###,
        "end|visible:late|",
    );
}

#[test]
fn output_handler_reads_global() {
    assert_shutdown_contract(
        r###"$saved='visible';ob_start(function($text){return $text.$GLOBALS['saved'].'|';});echo 'body|';"###,
        "body|visible|",
    );
}

#[test]
fn direct_global_reference() {
    assert_shutdown_contract(
        r###"$saved='visible';$alias=&$saved;function source(){try{yield 3;}finally{global $saved;echo $saved,'|';}}$it=source();$it->current();echo 'end|';"###,
        "end|visible|",
    );
}

#[test]
fn late_warning_location() {
    assert_shutdown_contract(
        r###"function source(){try{yield 3;}finally{echo $GLOBALS['absent'],'|';}}$it=source();$it->current();set_exception_handler(function($e)use($it){});echo 'end|';"###,
        "end|\nWarning: Undefined global variable $absent in Command line code on line 1\n|",
    );
}

#[test]
fn handler_global_object_with_destructor() {
    assert_shutdown_contract(
        r###"class Sample{public $value=29;function __destruct(){echo 'drop|';}}$saved=new Sample;function inspect(){echo isset($GLOBALS['saved'])?'present|':'absent|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|drop|absent|",
    );
}

#[test]
fn array_destructor_order() {
    assert_shutdown_contract(
        r###"class Sample{function __construct(public $name){}function __destruct(){echo $this->name,'|';}}$later=[new Sample('first')];$earlier=[new Sample('second')];function inspect(){echo 'finally|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|first|second|finally|",
    );
}

#[test]
fn reference_destructor() {
    assert_shutdown_contract(
        r###"class Sample{public $value=29;function __destruct(){echo 'drop|';}}$saved=new Sample;$alias=&$saved;function inspect(){echo $GLOBALS['saved']->value,'|';};function source(){try{yield 3;}finally{inspect();}}$it=source();$it->current();set_error_handler(function()use($it){});echo 'end|';"###,
        "end|drop|29|",
    );
}

#[test]
fn array_and_handler_shared_object() {
    assert_shutdown_contract(
        r###"class Sample{function __destruct(){echo 'drop|';}}$saved=[new Sample];function source(){try{yield 3;}finally{echo 'finally|';}}$it=source();$it->current();set_error_handler(function()use($it,$saved){});echo 'end|';"###,
        "end|drop|finally|",
    );
}

#[test]
fn missing_global_source() {
    assert_shutdown_contract(
        r###"echo $GLOBALS['absent'],'|';"###,
        "\nWarning: Undefined global variable $absent in Command line code on line 1\n|",
    );
}

#[test]
fn global_promoted_by_callback() {
    assert_shutdown_contract(
        r###"class Owner{public $item;}class Item{static $pin;function __destruct(){global $owner;echo 'drop:',gettype($owner),'|';if($owner)$owner->item=null;}}$owner=new Owner;$owner->item=new Item;Item::$pin=&$owner->item;function replace($x){$x->item=new Item;}replace($owner);echo 'end|';"###,
        "drop:object|drop:object|end|",
    );
}

#[test]
fn global_promoted_no_static() {
    assert_shutdown_contract(
        r###"class Owner{public $item;}class Item{function __destruct(){global $owner;echo 'drop:',gettype($owner),'|';if($owner)$owner->item=null;}}$owner=new Owner;$owner->item=new Item;function replace($x){$x->item=new Item;}replace($owner);echo 'end|';"###,
        "drop:object|drop:object|end|",
    );
}

#[test]
fn global_promoted_explicit() {
    assert_shutdown_contract(
        r###"class Owner{public $item;}class Item{static $pin;function __destruct(){global $owner;echo 'drop:',gettype($owner),'|';if($owner)$owner->item=null;}}$owner=new Owner;$alias=&$owner;$owner->item=new Item;Item::$pin=&$owner->item;function replace($x){$x->item=new Item;}replace($owner);echo 'end|';"###,
        "drop:object|drop:object|end|",
    );
}

#[test]
fn static_child() {
    assert_shutdown_contract(
        r###"class Owner{public $item;}class Item{static $pin;function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$owner=new Owner;$owner->item=new Item;Item::$pin=$owner->item;echo 'end|';"###,
        "end|absent|",
    );
}

#[test]
fn static_ref_child() {
    assert_shutdown_contract(
        r###"class Owner{public $item;}class Item{static $pin;function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$owner=new Owner;$owner->item=new Item;Item::$pin=&$owner->item;echo 'end|';"###,
        "end|absent|",
    );
}

#[test]
fn static_global() {
    assert_shutdown_contract(
        r###"class Store{static $item;} $owner=(object)['v'=>71];Store::$item=$owner;register_shutdown_function(function(){echo 'shutdown|';});function stream(){try{yield 1;}finally{echo isset($GLOBALS['owner'])?'present|':'absent|';}}$stream=stream();$stream->current();set_error_handler(function()use($stream){});echo 'end|';"###,
        "end|shutdown|present|",
    );
}

#[test]
fn direct_reference_fiber() {
    assert_shutdown_contract(
        r###"function work(){try{echo 'start|';Fiber::suspend();yield 1;}finally{echo 'final|';}}$iterator=work();$worker=new Fiber(function()use($iterator,&$worker){$iterator->current();});$worker->start();echo 'end|';"###,
        "start|end|final|",
    );
}

#[test]
fn array_reference_fiber() {
    assert_shutdown_contract(
        r###"function work(){try{echo 'start|';Fiber::suspend();yield 1;}finally{echo 'final|';}}$iterator=work();$worker=new Fiber(function()use($iterator,&$worker){$iterator->current();});$roots=[$worker];$worker->start();echo 'end|';"###,
        "start|end|final|",
    );
}

#[test]
fn handler_reference_fiber() {
    assert_shutdown_contract(
        r###"function work(){try{echo 'start|';Fiber::suspend();yield 1;}finally{echo 'final|';}}$iterator=work();$worker=new Fiber(function()use($iterator,&$worker){$iterator->current();});set_error_handler(function()use($worker){});$worker->start();echo 'end|';"###,
        "start|end|final|",
    );
}

#[test]
fn object_identity_at_final() {
    assert_shutdown_contract(
        r###"class Sample{function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$owner=new Sample;$alias=$owner;echo 'end|';"###,
        "end|present|",
    );
}

#[test]
fn direct_reference_at_final() {
    assert_shutdown_contract(
        r###"class Sample{function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$owner=new Sample;$alias=&$owner;echo 'end|';"###,
        "end|present|",
    );
}

#[test]
fn direct_array_at_final() {
    assert_shutdown_contract(
        r###"class Sample{function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$owner=new Sample;$array=[$owner];echo 'end|';"###,
        "end|present|",
    );
}

#[test]
fn callback_assignment_result_untyped() {
    assert_shutdown_contract(
        r###"class Slot{public $item;}class Entry{static ?Entry $link;public $label='v';function __destruct(){global $slot;echo 'drop:',gettype($slot),'|';$slot->item=null;}}function change($slot){echo ($slot->item=new Entry)->label,'|';}$slot=new Slot;$slot->item=new Entry;Entry::$link=&$slot->item;change($slot);change($slot);echo 'end|';"###,
        "drop:object|drop:object|v|v|end|drop:object|",
    );
}

#[test]
fn callback_assignment_result_typed() {
    assert_shutdown_contract(
        r###"class Slot{public ?Entry $item;}class Entry{static ?Entry $link;public $label='v';function __destruct(){global $slot;echo 'drop:',gettype($slot),'|';$slot->item=null;}}function change($slot){echo ($slot->item=new Entry)->label,'|';}$slot=new Slot;$slot->item=new Entry;Entry::$link=&$slot->item;change($slot);change($slot);echo 'end|';"###,
        "drop:object|drop:object|v|v|end|drop:object|",
    );
}

#[test]
fn dynamic_object() {
    assert_shutdown_contract(
        r###"$key='dy'.'namic';$GLOBALS[$key]=(object)['v'=>71];function stream(){try{yield 1;}finally{echo isset($GLOBALS['dynamic'])?'present|':'absent|';}}$stream=stream();$stream->current();set_error_handler(function()use($stream){});echo 'end|';"###,
        "end|absent|",
    );
}

#[test]
fn dynamic_closure() {
    assert_shutdown_contract(
        r###"$key='dy'.'namic';$GLOBALS[$key]=fn()=>1;function stream(){try{yield 1;}finally{echo isset($GLOBALS['dynamic'])?'present|':'absent|';}}$stream=stream();$stream->current();set_error_handler(function()use($stream){});echo 'end|';"###,
        "end|absent|",
    );
}

#[test]
fn dynamic_shared() {
    assert_shutdown_contract(
        r###"$key='dy'.'namic';$GLOBALS[$key]=(object)['v'=>71];class Store{static $pin;}Store::$pin=$GLOBALS[$key];function stream(){try{yield 1;}finally{echo isset($GLOBALS['dynamic'])?'present|':'absent|';}}$stream=stream();$stream->current();set_error_handler(function()use($stream){});echo 'end|';"###,
        "end|present|",
    );
}

#[test]
fn dynamic_ref() {
    assert_shutdown_contract(
        r###"$key='dy'.'namic';$GLOBALS[$key]=(object)['v'=>71];$alias=&$GLOBALS[$key];function stream(){try{yield 1;}finally{echo isset($GLOBALS['dynamic'])?'present|':'absent|';}}$stream=stream();$stream->current();set_error_handler(function()use($stream){});echo 'end|';"###,
        "end|present|",
    );
}

#[test]
fn self_global_cv_single() {
    assert_shutdown_contract(
        r###"class Note{function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$owner=new Note;echo 'end|';"###,
        "end|absent|",
    );
}

#[test]
fn self_global_cv_shared() {
    assert_shutdown_contract(
        r###"class Note{function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$owner=new Note;$alias=$GLOBALS['owner'];echo 'end|';"###,
        "end|present|",
    );
}

#[test]
fn self_global_dynamic_single() {
    assert_shutdown_contract(
        r###"class Note{function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$key='own'.'er';$GLOBALS[$key]=new Note;echo 'end|';"###,
        "end|absent|",
    );
}

#[test]
fn self_global_dynamic_shared() {
    assert_shutdown_contract(
        r###"class Note{function __destruct(){echo isset($GLOBALS['owner'])?'present|':'absent|';}}$key='own'.'er';$GLOBALS[$key]=new Note;$alias=$GLOBALS['owner'];echo 'end|';"###,
        "end|present|",
    );
}

#[test]
fn stringable_global_direct() {
    assert_shutdown_contract(
        r###"class Holder{public string $text='';}class Render{function __toString(){global $holder;$holder->text='nested';$holder->text.=' write';return 'outer';}}$holder=new Holder;$holder->text=new Render;echo $holder->text,'|';$holder=new Holder;$text=&$holder->text;$holder->text=new Render;echo $text,'|';"###,
        "outer|outer|",
    );
}

#[test]
fn stringable_global_alias() {
    assert_shutdown_contract(
        r###"class Holder{public string $text='';}class Render{function __toString(){global $holder;$holder->text='nested';$holder->text.=' write';return 'outer';}}$holder=new Holder;$reference=&$holder;$holder->text=new Render;echo $holder->text,'|';$holder=new Holder;$text=&$holder->text;$holder->text=new Render;echo $text,'|';"###,
        "outer|outer|",
    );
}

#[test]
fn child_destructor_observes_the_retired_owner_symbol() {
    assert_shutdown_error(
        "class Bucket{public $item;}class FinalItem{function __destruct(){$GLOBALS['bucket']->item=null;}}$bucket=new Bucket;$bucket->item=new FinalItem;echo 'end|';",
        "end|",
        "Fatal error: Uncaught Error: Attempt to assign property \"item\" on null in Command line code:1\nStack trace:\n#0 [internal function]: FinalItem->__destruct()\n#1 {main}\n  thrown in Command line code on line 1\n",
    );
}

#[test]
fn surviving_array_destructor_keeps_its_internal_trace_origin() {
    assert_shutdown_error(
        "class Late{function __destruct(){global $pool;$pool->field=null;}}$pool=[new Late];try{unavailable_shutdown_probe($pool);}catch(Error $error){echo 'caught|';}",
        "caught|",
        "Fatal error: Uncaught Error: Attempt to assign property \"field\" on array in Command line code:1\nStack trace:\n#0 [internal function]: Late->__destruct()\n#1 {main}\n  thrown in Command line code on line 1\n",
    );
}

fn assert_shutdown_error(source: &str, stdout: &str, stderr: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=stderr",
            "-d",
            "log_errors=0",
            "-r",
            source,
        ])
        .output()
        .expect("run shutdown error contract");
    assert_eq!(output.status.code(), Some(255), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), stdout);
    assert_eq!(String::from_utf8(output.stderr).unwrap(), stderr);
}

fn assert_shutdown_contract(source: &str, expected: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0", "-r", source])
        .output()
        .expect("run late shutdown symbol contract");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}
