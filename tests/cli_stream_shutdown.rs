#![cfg(feature = "stream-registry")]

use std::fs::OpenOptions;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(0);

fn capture(fixture: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "rphp-stream-shutdown-{}-{}",
        std::process::id(),
        NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed)
    ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    // Both descriptors share one file offset: checking separate stdout/stderr
    // strings would miss a close callback emitted before the fatal diagnostic.
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/user_stream_filters");
    let status = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "fatal_error_backtraces=0",
        ])
        .arg(root.join(fixture))
        .stdout(Stdio::from(file.try_clone().unwrap()))
        .stderr(Stdio::from(file))
        .status()
        .unwrap();
    let output = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(status.code(), Some(255), "{output}");
    output.replace(root.to_str().unwrap(), "[fixtures]")
}

#[test]
fn uncaught_parse_diagnostic_precedes_request_resource_close() {
    assert_eq!(
        capture("shutdown_parse.php"),
        concat!(
            "before compilation\n\n",
            "Parse error: syntax error, unexpected identifier \"expression\" in [fixtures]/shutdown_invalid.inc on line 1\n",
            "pending input closed\n"
        )
    );
}

#[test]
fn fatal_nested_read_preserves_each_request_close_bailout() {
    assert_eq!(
        capture("shutdown_bailout.php"),
        concat!(
            "nested read\n\n",
            "Fatal error: read stopped in [fixtures]/shutdown_bailout.php on line 15\n\n",
            "Fatal error: close stopped in [fixtures]/shutdown_bailout.php on line 17\n\n",
            "Fatal error: close stopped in [fixtures]/shutdown_bailout.php on line 17\n"
        )
    );
}
