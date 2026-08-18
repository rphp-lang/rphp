use std::process::Command;

fn run(arguments: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(arguments)
        .output()
        .expect("rphp subprocess should start");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn startup_assertion_modes_skip_arguments_and_allow_runtime_toggle() {
    let source = r#"
function probe() { echo "evaluated\n"; return false; }
var_dump(assert(probe()));
var_dump(ini_get("zend.assertions"));
"#;
    for (mode, expected) in [
        ("-1", "bool(true)\nstring(2) \"-1\"\n"),
        ("0", "bool(true)\nstring(1) \"0\"\n"),
    ] {
        let (status, stdout, stderr) =
            run(&["-d", &format!("zend.assertions={mode}"), "-r", source]);
        assert_eq!(status, 0);
        assert_eq!(stdout, expected);
        assert_eq!(stderr, "");
    }

    let (status, stdout, stderr) = run(&[
        "-dzend.assertions=0",
        "-dassert.exception=1",
        "-r",
        r#"
function probe() { echo "evaluated\n"; return false; }
var_dump(assert(probe()));
var_dump(ini_set("zend.assertions", 1));
try { assert(probe()); } catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
"#,
    ]);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "bool(true)\nstring(1) \"0\"\nevaluated\nassert(probe())\n"
    );
    assert_eq!(stderr, "");
}

#[test]
fn completely_eliminated_assertion_still_marks_a_generator() {
    let (status, stdout, stderr) = run(&[
        "-d",
        "zend.assertions=-1",
        "-r",
        r#"
function values() { assert(yield 1); return null; }
var_dump(values() instanceof Generator);
"#,
    ]);
    assert_eq!(status, 0);
    assert_eq!(stdout, "bool(true)\n");
    assert_eq!(stderr, "");
}

#[test]
fn completely_eliminated_assertions_cannot_be_enabled_at_runtime() {
    let (status, stdout, stderr) = run(&[
        "-dzend.assertions=-1",
        "-r",
        r#"var_dump(ini_set("zend.assertions", 0)); var_dump(ini_get("zend.assertions"));"#,
    ]);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "\nWarning: zend.assertions may be completely enabled or disabled only in php.ini in Command line code on line 1\nbool(false)\nstring(2) \"-1\"\n"
    );
    assert_eq!(stderr, "");
}
