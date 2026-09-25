use std::process::Command;

fn assert_startup_lifecycle(source: &str, expected: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-dzend.enable_gc=0",
            "-r",
            source,
        ])
        .output()
        .expect("run startup-disabled lifecycle probe");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn cyclic_owner() {
    assert_startup_lifecycle(
        r#"class End {public $self;function __construct(){$this->self=$this;}function __destruct(){echo "drop|";}} new End; echo "body|";"#,
        "body|drop|",
    );
}

#[test]
fn array_child() {
    assert_startup_lifecycle(
        r#"class Leaf {function __destruct(){echo "leaf|";}} $a=[new Leaf];$a[]=&$a;unset($a);echo "body|";"#,
        "body|leaf|",
    );
}

#[test]
fn enable_old() {
    assert_startup_lifecycle(
        r#"class End {public $self;function __construct(){$this->self=$this;}function __destruct(){echo "drop|";}} new End;gc_enable();echo gc_collect_cycles(),"|body|";"#,
        "0|body|drop|",
    );
}

#[test]
fn weak_map() {
    assert_startup_lifecycle(
        r#"$w=new WeakMap;$a=new stdClass;$a->a=$a;$w[$a]=17;unset($a);echo gc_collect_cycles(),"/",count($w),"|";gc_enable();echo gc_collect_cycles(),"/",count($w);"#,
        "0/1|0/1",
    );
}

#[test]
fn weak_reference() {
    assert_startup_lifecycle(
        r#"$a=new stdClass;$a->a=$a;$w=WeakReference::create($a);unset($a);echo gc_collect_cycles(),"/",(int)($w->get()!==null),"|";gc_enable();echo gc_collect_cycles(),"/",(int)($w->get()!==null);"#,
        "0/1|0/1",
    );
}

#[test]
fn later_real_release() {
    assert_startup_lifecycle(
        r#"$w=new WeakMap;$a=new stdClass;$a->a=$a;$w[$a]=17;$r=WeakReference::create($a);unset($a);gc_enable();echo gc_collect_cycles(),"/",count($w),"|";$a=$r->get();unset($a);echo gc_collect_cycles(),"/",count($w);"#,
        "0/1|1/0",
    );
}

#[test]
fn creation_order() {
    assert_startup_lifecycle(
        r#"class End {public $self;function __construct(public $name){$this->self=$this;}function __destruct(){echo $this->name,"|";}}new End("first");new End("second");new End("third");echo "body|";"#,
        "body|first|second|third|",
    );
}

#[test]
fn closure_child() {
    assert_startup_lifecycle(
        r#"class Leaf {function __destruct(){echo "leaf|";}}$leaf=new Leaf;$call=null;$call=function()use(&$call,$leaf){};unset($call,$leaf);echo "body|";"#,
        "body|leaf|",
    );
}

#[test]
fn resurrect_and_create() {
    assert_startup_lifecycle(
        r#"class End {public $self;function __construct(public $name){$this->self=$this;}function __destruct(){echo $this->name,"|";if($this->name==="first"){$GLOBALS["saved"]=$this;new End("second");}}}new End("first");echo "body|";"#,
        "body|first|second|",
    );
}

#[test]
fn enable_during_shutdown() {
    assert_startup_lifecycle(
        r#"class Child {public $self;function __construct(){$this->self=$this;}function __destruct(){echo "child|";}}class End {public $self;function __construct(){$this->self=$this;}function __destruct(){echo "end|";gc_enable();new Child;echo gc_collect_cycles(),"|";}}new End;echo "body|";"#,
        "body|end|child|1|",
    );
}
