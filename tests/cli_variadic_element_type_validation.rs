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
fn weak_variadic_elements_are_coerced_before_positional_named_and_unpacked_packing() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
function stringify(string ...$values): string { return implode(',', $values); }
function replace(string &...$values): void { $values[0] = strtoupper($values[0]); }
class Formatter {
    public function render(string ...$values): string { return implode(',', $values); }
}
$referenced = 6;
replace($referenced);
echo stringify(1, true, 3.5), '|';
echo stringify(...[4, false]), '|';
echo stringify(named: 5), '|';
echo (new Formatter())->render(7, 8), '|', gettype($referenced), ':', $referenced;
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "1,1,3.5|4,|5|7,8|string:6");
    assert_eq!(stderr, "");
}

#[test]
fn invalid_variadic_elements_report_the_actual_argument_and_remain_catchable() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nfunction acceptArrays(array ...$values): void {}\nacceptArrays([], 2);\n",
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "\nFatal error: Uncaught TypeError: acceptArrays(): Argument #2 must be of type array, int given, called in Standard input code on line 3 and defined in Standard input code:2\nStack trace:\n#0 Standard input code(3): acceptArrays(Array, 2)\n#1 {main}\n  thrown in Standard input code on line 2\n"
    );

    let (status, stdout, stderr) = run_stdin(
        "<?php\nfunction acceptArrays(array ...$values): void {}\ntry { acceptArrays([], 2); } catch (TypeError $error) { echo $error->getMessage(); }\n",
    );
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "acceptArrays(): Argument #2 must be of type array, int given, called in Standard input code on line 3"
    );
    assert_eq!(stderr, "");
}
