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
fn invalid_loop_control_reports_the_php_compile_fatal_at_its_source_line() {
    for (statement, message) in [
        ("break;", "'break' not in the 'loop' or 'switch' context"),
        ("break 1;", "'break' not in the 'loop' or 'switch' context"),
        (
            "continue;",
            "'continue' not in the 'loop' or 'switch' context",
        ),
        (
            "continue 1;",
            "'continue' not in the 'loop' or 'switch' context",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(&format!(
            "<?php\nfunction invalid() {{\n    {statement}\n}}\n"
        ));
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            format!("Fatal error: {message} in Standard input code on line 3\n"),
            "statement: {statement}"
        );
    }
}

#[test]
fn excessive_loop_control_levels_report_the_requested_depth() {
    for (statement, message) in [
        ("break 2;", "Cannot 'break' 2 levels"),
        ("continue 2;", "Cannot 'continue' 2 levels"),
        ("break 2147483648;", "Cannot 'break' 2147483648 levels"),
    ] {
        let (status, stdout, stderr) =
            run_stdin(&format!("<?php\nwhile (true) {{\n    {statement}\n}}\n"));
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            format!("Fatal error: {message} in Standard input code on line 3\n"),
            "statement: {statement}"
        );
    }
}

#[test]
fn invalid_loop_control_operands_are_located_compile_fatals() {
    for (statement, message) in [
        (
            "break 0;",
            "'break' operator accepts only positive integers",
        ),
        (
            "continue 0;",
            "'continue' operator accepts only positive integers",
        ),
        (
            "break \"2\";",
            "'break' operator accepts only positive integers",
        ),
        (
            "break -1;",
            "'break' operator with non-integer operand is no longer supported",
        ),
        (
            "break -0;",
            "'break' operator with non-integer operand is no longer supported",
        ),
        (
            "continue (-0.0);",
            "'continue' operator with non-integer operand is no longer supported",
        ),
        (
            "break $depth;",
            "'break' operator with non-integer operand is no longer supported",
        ),
        (
            "continue 1 + 1;",
            "'continue' operator with non-integer operand is no longer supported",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(&format!(
            "<?php\nfunction invalid() {{\n    {statement}\n}}\n"
        ));
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            format!("Fatal error: {message} in Standard input code on line 3\n"),
            "statement: {statement}"
        );
    }
}

#[test]
fn valid_multilevel_loop_control_keeps_its_existing_execution_path() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nfor ($i = 0; $i < 3; $i++) {\n    while (true) { break ((2)); }\n}\necho 'after';\n",
    );
    assert_eq!(status, 0);
    assert_eq!(stdout, "after");
    assert_eq!(stderr, "");
}
