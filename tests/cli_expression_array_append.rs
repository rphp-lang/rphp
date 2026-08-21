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

#[test]
fn call_result_append_observes_return_reference_identity() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\n\
$items = [1];\n\
function copy_items(&$value) { return $value; }\n\
function &alias_items(&$value) { return $value; }\n\
copy_items($items)[] = 99;\n\
alias_items($items)[] = 2;\n\
$dynamic = 'alias_items';\n\
$dynamic($items)[] = 3;\n\
class AppendStore {\n\
    public $detached = [];\n\
    public static $shared = [];\n\
    public static function &shared() { return self::$shared; }\n\
    public function value() { return [7]; }\n\
}\n\
AppendStore::shared()[] = 4;\n\
(new AppendStore)->value()[] = 8;\n\
$store = new AppendStore;\n\
(new ReflectionProperty(AppendStore::class, 'detached'))->getValue($store)[] = 5;\n\
var_dump($items, AppendStore::$shared, $store->detached);\n",
    );
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "array(3) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(2)\n  [2]=>\n  int(3)\n}\narray(1) {\n  [0]=>\n  int(4)\n}\narray(0) {\n}\n"
    );
    assert_eq!(stderr, "");
}

#[test]
fn nested_append_evaluates_once_and_publishes_null_before_property_error() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\n\
$root = [];\n\
$events = [];\n\
function &append_target(&$root, &$events) {\n\
    $events[] = 'target';\n\
    return $root;\n\
}\n\
function append_value(&$events) {\n\
    $events[] = 'value';\n\
    return 7;\n\
}\n\
append_target($root, $events)[][] = append_value($events);\n\
try {\n\
    $root[][]->missing = 9;\n\
} catch (Error $error) {\n\
    echo $error->getMessage(), \"\\n\";\n\
}\n\
var_dump($events, $root);\n",
    );
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "Attempt to assign property \"missing\" on null\narray(2) {\n  [0]=>\n  string(6) \"target\"\n  [1]=>\n  string(5) \"value\"\n}\narray(2) {\n  [0]=>\n  array(1) {\n    [0]=>\n    int(7)\n  }\n  [1]=>\n  array(1) {\n    [0]=>\n    NULL\n  }\n}\n"
    );
    assert_eq!(stderr, "");
}

#[test]
fn non_call_temporary_and_nullsafe_append_remain_compile_errors() {
    let (status, stdout, stderr) = run_stdin("<?php\n[1]\n    [] = 2;\necho 'unreachable';\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Fatal error: Cannot use temporary expression in write context in Standard input code on line 2\n"
    );

    let (status, stdout, stderr) = run_stdin(
        "<?php\nclass AppendSource { function values() { return []; } }\n$source = null;\n$source?->values()[] = 1;\necho 'unreachable';\n",
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Fatal error: Can't use nullsafe operator in write context in Standard input code on line 4\n"
    );
}
