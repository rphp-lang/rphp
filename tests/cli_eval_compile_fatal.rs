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
fn suppressed_eval_compile_fatal_bypasses_catch_and_keeps_shutdown_mask() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
error_reporting(E_ALL);
register_shutdown_function(function () { echo "shutdown:", error_reporting(), "\n"; });
try { @eval('class self {}'); } catch (Throwable $error) { echo "caught\n"; }
echo "after\n";
"#,
    );

    assert_eq!(status, 255);
    assert_eq!(stdout, "shutdown:4437\n");
    assert_eq!(
        stderr,
        "\nFatal error: Cannot use \"self\" as a class name as it is reserved in Standard input code(4) : eval()'d code on line 1\n"
    );
}
