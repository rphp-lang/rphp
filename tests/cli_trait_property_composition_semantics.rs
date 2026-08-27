use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

fn assert_link_fatal(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("\nFatal error: {message} in Standard input code on line {line}\n")
    );
}

fn incompatible(first: &str, second: &str, property: &str, class: &str) -> String {
    format!(
        "{first} and {second} define the same property (${property}) in the composition of {class}. \
         However, the definition differs and is considered incompatible. Class was composed"
    )
}

#[test]
fn class_and_trait_properties_require_exactly_compatible_definitions() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\ntrait Same { public int $value = 1; }\nclass Compatible { use Same; public int $value = 1; }\necho (new Compatible())->value;\n",
    );
    assert_eq!((status, stdout.as_str(), stderr.as_str()), (0, "1", ""));

    assert_link_fatal(
        "<?php\ntrait Different { public $value = 1; }\nclass Incompatible { use Different; public $value = 2; }\n",
        &incompatible("Incompatible", "Different", "value", "Incompatible"),
        3,
    );
}

#[test]
fn storage_kind_participates_in_class_and_trait_compatibility() {
    assert_link_fatal(
        "<?php\ntrait StaticState { public static $value = 1; }\nclass InstanceConsumer { use StaticState; public $value = 1; }\n",
        &incompatible(
            "InstanceConsumer",
            "StaticState",
            "value",
            "InstanceConsumer",
        ),
        3,
    );
    assert_link_fatal(
        "<?php\ntrait InstanceState { public $value = 1; }\ntrait StaticState { public static $value = 1; }\nclass MixedConsumer { use InstanceState, StaticState; }\n",
        &incompatible("InstanceState", "StaticState", "value", "MixedConsumer"),
        4,
    );
}

#[test]
fn inherited_private_and_trait_private_properties_keep_distinct_slots() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
class ParentState {
    private $value = 'parent';
    public function parentValue() { return $this->value; }
}
trait ChildState {
    private $value = 'child';
    public function childValue() { return $this->value; }
}
class Consumer extends ParentState { use ChildState; }
$consumer = new Consumer();
echo $consumer->parentValue(), ':', $consumer->childValue();
"#,
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "parent:child");
    assert_eq!(stderr, "");
}

#[test]
fn inherited_trait_self_and_static_property_scopes_remain_distinct() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait Storage {
    public static $value;
    public static function storageSelf() { return self::$value; }
    public static function storageStatic() { return static::$value; }
}
trait Observer {
    public static function observerSelf() { return self::$value; }
    public static function observerStatic() { return static::$value; }
}
class Base { use Storage, Observer; }
class Child extends Base { use Storage; }
Base::$value = 'base';
Child::$value = 'child';
echo Base::storageSelf(), ':', Base::storageStatic(), '|';
echo Child::storageSelf(), ':', Child::storageStatic(), '|';
echo Base::observerSelf(), '|';
echo cHiLd::observerSelf(), ':', cHiLd::observerStatic();
"#,
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "base:base|child:child|base|base:child");
    assert_eq!(stderr, "");
}
