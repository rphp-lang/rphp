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
fn invalid_readonly_declarations_match_php_diagnostic_channels_and_status() {
    for (source, expected) in [
        (
            "<?php\nreadonly readonly class Example {}\n",
            "Fatal error: Multiple readonly modifiers are not allowed in Standard input code on line 2\n",
        ),
        (
            "<?php\nreadonly enum Example {}\n",
            "Parse error: syntax error, unexpected token \"enum\", expecting \"abstract\" or \"final\" or \"readonly\" or \"class\" in Standard input code on line 2\n",
        ),
        (
            "<?php\nreadonly interface Example {}\n",
            "Parse error: syntax error, unexpected token \"interface\", expecting \"abstract\" or \"final\" or \"readonly\" or \"class\" in Standard input code on line 2\n",
        ),
        (
            "<?php\nreadonly trait Example {}\n",
            "Parse error: syntax error, unexpected token \"trait\", expecting \"abstract\" or \"final\" or \"readonly\" or \"class\" in Standard input code on line 2\n",
        ),
        (
            "<?php\nclass Example {\n    readonly const VALUE = 1;\n}\n",
            "Fatal error: Cannot use the readonly modifier on a class constant in Standard input code on line 3\n",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn valid_readonly_classes_properties_and_classlike_neighbors_stay_executable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
readonly class Box {
    public function __construct(public int $value) {}
}
final readonly class FinalBox {}
abstract readonly class AbstractBox {}
enum NeighboringEnum { case Value; }
interface NeighboringInterface {}
trait NeighboringTrait {}
echo (new Box(42))->value;
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "42");
    assert_eq!(stderr, "");
}
