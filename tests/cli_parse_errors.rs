use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
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
        String::from_utf8(output.stderr).expect("diagnostic should be UTF-8"),
    )
}

#[test]
fn uncaught_eval_parse_error_uses_the_parse_diagnostic_envelope() {
    let (status, stderr) = run_stdin("<?php\neval(\"<<<'end'\\n  \" );\n");

    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "\nParse error: syntax error, unexpected end of file, expecting variable or heredoc end or \"${\" or \"{$\" in Standard input code(2) : eval()'d code on line 2\n"
    );
}

#[test]
fn user_parse_error_subclasses_keep_the_uncaught_throwable_envelope() {
    let (status, stderr) = run_stdin(
        "<?php\nclass CustomParseError extends ParseError {}\nthrow new CustomParseError('x');\n",
    );

    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "\nFatal error: Uncaught CustomParseError: x in Standard input code:3\nStack trace:\n#0 {main}\n  thrown in Standard input code on line 3\n"
    );
}

#[test]
fn source_unit_parse_errors_use_php_failure_status() {
    let (status, stderr) = run_stdin("<?php\n$value = factory<<<DOC\nbody\nDOC;\n");

    assert_eq!(status, 255);
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected heredoc start \"<<<DOC\" in Standard input code on line 2\n"
    );
}
