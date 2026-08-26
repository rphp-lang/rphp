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

fn assert_compile_fatal(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("Fatal error: {message} in Standard input code on line {line}\n")
    );
}

#[test]
fn duplicate_switch_and_match_defaults_are_located_compile_fatals() {
    for (source, message, line) in [
        (
            "<?php\necho 'unreachable';\nswitch (missing()) {\n    default: break;\n    case 1: break;\n    default: break;\n}\n",
            "Switch statements may only contain one default clause",
            6,
        ),
        (
            "<?php\necho 'unreachable';\n$value = match (missing()) {\n    default => 'first',\n    1 => 'one',\n    default => 'second',\n};\n",
            "Match expressions may only contain one default arm",
            6,
        ),
        (
            "<?php\nswitch (1):\n    default: break;\n    default: break;\nendswitch;\n",
            "Switch statements may only contain one default clause",
            4,
        ),
        (
            "<?php\n$value = match (1) {\n    default => match (2) {\n        default => 'inner first',\n        default => 'inner second',\n    },\n};\n",
            "Match expressions may only contain one default arm",
            5,
        ),
    ] {
        assert_compile_fatal(source, message, line);
    }
}

#[test]
fn defaults_are_counted_independently_for_nested_constructs() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\n$value = match (1) {\n    default => match (2) { default => 'nested' },\n};\nswitch ($value) {\n    default:\n        switch (1) { default: echo $value; break; }\n}\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "nested");
    assert_eq!(stderr, "");
}
