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
fn unparenthesized_nested_ternaries_have_distinct_compile_fatals() {
    for (expression, message) in [
        (
            "$condition ? 2 : 3 ? 4 : 5",
            "Unparenthesized `a ? b : c ? d : e` is not supported. Use either `(a ? b : c) ? d : e` or `a ? b : (c ? d : e)`",
        ),
        (
            "$condition ? 2 : 3 ?: 4",
            "Unparenthesized `a ? b : c ?: d` is not supported. Use either `(a ? b : c) ?: d` or `a ? b : (c ?: d)`",
        ),
    ] {
        let source = format!("<?php\n$condition = true;\n{expression};\n");
        let (status, stdout, stderr) = run_stdin(&source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(
            stderr,
            format!("Fatal error: {message} in Standard input code on line 3\n")
        );
    }
}

#[test]
fn explicitly_parenthesized_nested_ternaries_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
echo (1 ? 2 : 3) ? 4 : 5, '|';
echo 0 ? 2 : (3 ? 4 : 5), '|';
echo (1 ? 2 : 3) ?: 4, '|';
echo 0 ? 2 : (3 ?: 4);
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "4|4|2|3");
    assert_eq!(stderr, "");
}
