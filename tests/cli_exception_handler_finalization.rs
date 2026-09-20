use std::fs::OpenOptions;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(0);

fn run_stdin(source: &str) -> Output {
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
    child.wait_with_output().expect("rphp should finish")
}

fn run_stdin_combined(source: &str) -> (Option<i32>, Vec<u8>) {
    let path = std::env::temp_dir().join(format!(
        "rphp-exception-finalization-{}-{}",
        std::process::id(),
        NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed)
    ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("combined capture should be created");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::from(
            file.try_clone().expect("capture clone should succeed"),
        ))
        .stderr(Stdio::from(file))
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let status = child.wait().expect("rphp should finish");
    let output = std::fs::read(&path).expect("combined output should be readable");
    std::fs::remove_file(path).expect("combined capture should be removed");
    (status.code(), output)
}

#[test]
fn replacement_exception_handler_observes_the_handler_exception() {
    let output = run_stdin(
        r#"<?php
set_exception_handler(function (Throwable $error): void {
    echo 'first:', $error->getMessage(), '|';
    set_exception_handler(function (Throwable $replacement): void {
        echo 'second:', $replacement->getMessage(), '|';
    });
    throw new Exception('replacement');
});
throw new Exception('main');
"#,
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"first:main|second:replacement|");
    assert!(output.stderr.is_empty());
}

#[test]
fn pending_uncaught_exception_still_runs_shutdown_callbacks() {
    let output = run_stdin(
        r#"<?php
register_shutdown_function(function (): void { echo 'shutdown|'; });
throw new Exception('main');
"#,
    );

    assert_eq!(output.status.code(), Some(255));
    assert_eq!(output.stdout, b"shutdown|");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(stderr.contains("Fatal error: Uncaught Exception: main"));
}

#[test]
fn handler_installed_during_shutdown_does_not_catch_the_existing_fatal() {
    let output = run_stdin(
        r#"<?php
register_shutdown_function(function (): void {
    echo 'shutdown|';
    set_exception_handler(function (Throwable $error): void {
        echo 'late:', $error->getMessage(), '|';
    });
});
throw new Exception('main');
"#,
    );

    assert_eq!(output.status.code(), Some(255));
    assert_eq!(output.stdout, b"shutdown|");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(stderr.contains("Fatal error: Uncaught Exception: main"));
}

#[test]
fn suspended_generator_finally_precedes_the_uncaught_fatal() {
    let (status, output) = run_stdin_combined(
        r#"<?php
function values(): Generator {
    try {
        yield 1;
        yield 2;
    } finally {
        echo "finally\n";
    }
}
foreach (values() as $value) {
    echo $value, "\n";
    throw new Exception('stop');
}
"#,
    );

    assert_eq!(status, Some(255));
    let output = String::from_utf8(output).expect("combined output should be UTF-8");
    let finally = output
        .find("finally\n")
        .expect("finally output should exist");
    let fatal = output
        .find("Fatal error: Uncaught Exception: stop")
        .expect("fatal output should exist");
    assert!(finally < fatal, "{output}");
}

#[test]
fn final_output_handler_exception_uses_the_active_exception_handler() {
    let output = run_stdin(
        r#"<?php
set_exception_handler(function (Throwable $error): void {
    echo 'caught:', $error->getMessage(), '|';
});
ob_start(function (string $buffer): string {
    throw new Exception('buffer');
});
echo 'body';
"#,
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"bodycaught:buffer|");
    assert!(output.stderr.is_empty());
}

#[test]
fn chunk_handler_exception_interrupts_exit_and_disables_only_that_handler() {
    let output = run_stdin(
        r#"<?php
ob_start(function (string $buffer, int $phase): string {
    fwrite(STDOUT, "handler:$phase:$buffer|");
    throw new Exception('chunk');
}, 4);
try {
    exit('hello');
} catch (Throwable $error) {
    echo 'caught:', $error->getMessage(), '|level:', ob_get_level(), '|';
}
echo 'after|';
"#,
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        b"handler:1:hello|hellocaught:chunk|level:1|after|"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn header_after_unbuffered_output_reports_the_first_php_origin() {
    let output = run_stdin(
        r#"<?php
echo 'sent|';
header('X-Test: value');
"#,
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.starts_with("sent|\nWarning: Cannot modify header information"));
    assert!(stdout.contains("output started at Standard input code:2"));
    assert!(output.stderr.is_empty());
}
