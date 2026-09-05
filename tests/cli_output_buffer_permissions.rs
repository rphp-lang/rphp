use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run_locked_call(call: &str, stderr_display: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
    if stderr_display {
        command.args(["-n", "-d", "display_errors=stderr", "-d", "log_errors=0"]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    let source = format!(
        "<?php\nob_start(function ($bytes) {{\n    try {{ {call}; }} catch (Throwable $error) {{ echo 'must-not-catch'; }}\n    return $bytes;\n}});\necho 'pending';\nob_end_flush();\necho 'must-not-continue';\n"
    );
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("PHP source should be written");
    child.wait_with_output().expect("rphp should finish")
}

#[test]
fn locked_output_operations_report_exact_stderr_fatals_without_releasing_output() {
    for operation in [
        "ob_start",
        "ob_clean",
        "ob_flush",
        "ob_end_clean",
        "ob_end_flush",
        "ob_get_clean",
        "ob_get_flush",
    ] {
        let result = run_locked_call(&format!("{operation}()"), true);
        assert_eq!(result.status.code(), Some(255), "{operation}");
        assert!(result.stdout.is_empty(), "{operation}: {:?}", result.stdout);
        assert_eq!(
            result.stderr,
            format!("Fatal error: {operation}(): Cannot use output buffering in output buffering display handlers in Standard input code on line 3\n").as_bytes(),
            "{operation}"
        );
    }
}

#[test]
fn default_runtime_fatal_display_boundary_is_unchanged() {
    let result = run_locked_call("ob_get_clean()", false);
    assert_eq!(result.status.code(), Some(255));
    assert!(result.stdout.is_empty());
    assert_eq!(
        result.stderr,
        b"\nFatal error: ob_get_clean(): Cannot use output buffering in output buffering display handlers in Standard input code on line 3\n"
    );
}

#[test]
fn stderr_policy_also_applies_to_existing_non_ob_runtime_fatals() {
    let result = run_locked_call("highlight_string('<?php echo 1;', true)", true);
    assert_eq!(result.status.code(), Some(255));
    assert!(result.stdout.is_empty());
    assert_eq!(
        result.stderr,
        b"Fatal error: highlight_string(): Cannot use output buffering in output buffering display handlers in Standard input code on line 3\n"
    );
}
