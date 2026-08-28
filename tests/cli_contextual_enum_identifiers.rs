use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "log_errors=0",
            "-d",
            "html_errors=0",
            "-d",
            "fatal_error_backtraces=0",
        ])
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
fn direct_contextual_enum_identifiers_keep_statement_order() {
    let source = r#"<?php
namespace enum { function ping() { return 'qualified'; } }
namespace {
    function enum() { return 'direct'; }
    const enum = 'bare';
    echo enum(), '|', enum\ping(), '|', enum, "\n";
}
"#;
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 0, "{stderr:?}");
    assert_eq!(stdout, "direct|qualified|bare\n");
    assert!(stderr.is_empty(), "{stderr:?}");
}

#[test]
fn later_syntax_errors_still_suppress_contextual_enum_side_effects() {
    let source = "<?php\nfunction enum() { return 'never'; }\necho enum();\nisset(, $value);\n";
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{stderr:?}");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \",\" in Standard input code on line 4\n"
    );
}

#[test]
fn missing_enum_declaration_name_reports_the_block_line_without_output() {
    let source = "<?php\necho 'never';\nenum {}\n";
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255, "{stderr:?}");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert_eq!(
        stderr,
        "Parse error: syntax error, unexpected token \"{\" in Standard input code on line 3\n"
    );
}
