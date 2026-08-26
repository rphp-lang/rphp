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
fn foreach_key_reference_is_a_source_unit_compile_fatal() {
    for (source, line) in [
        ("<?php\nforeach ([1] as &$key => $value) {}\n", 2),
        (
            "<?php\n$items = [1];\nforeach (\n    $items as\n    &$key => $value\n) {}\n",
            4,
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            format!(
                "Fatal error: Key element cannot be a reference in Standard input code on line {line}\n"
            )
        );
    }
}

#[test]
fn foreach_value_references_and_ordinary_keys_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
$items = [1, 2];
foreach ($items as $key => &$value) { $value += $key + 1; }
unset($value);
foreach ($items as &$value) { $value *= 2; }
unset($value);
echo implode(',', $items);
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "4,8");
    assert_eq!(stderr, "");
}
