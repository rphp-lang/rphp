use std::io::Write;
use std::process::{Command, Stdio};

fn check(name: &str, expected: &str) {
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(format!(
            "{}/tests/fixtures/stream_read_projections/{name}.php",
            env!("CARGO_MANIFEST_DIR")
        ))
        .output()
        .expect("CLI starts");
    assert_eq!(result.status.code(), Some(0), "{name}: {:?}", result.stderr);
    assert_eq!(result.stderr, b"", "{name}");
    assert_eq!(result.stdout, expected.as_bytes(), "{name}");
}

macro_rules! specimen {
    ($name:ident) => {
        #[test]
        fn $name() {
            check(
                stringify!($name),
                include_str!(concat!(
                    "fixtures/stream_read_projections/",
                    stringify!($name),
                    ".out"
                )),
            );
        }
    };
}

specimen!(bytes);
specimen!(base64_boundary);
specimen!(passthrough);
specimen!(resources);
specimen!(metadata);
specimen!(read_error);
specimen!(prebuffer);
specimen!(output_order);
#[cfg(feature = "include-path")]
specimen!(include_source);
#[cfg(feature = "stream-registry")]
specimen!(filters);
#[cfg(feature = "stream-registry")]
specimen!(callback_failure);
#[cfg(feature = "stream-context")]
specimen!(wrong_resource);

#[test]
fn standard_input_to_output_keeps_raw_bytes_and_count() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-r",
            "echo fgetc(STDIN); $n = fpassthru(STDIN); echo '|', $n;",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI starts");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\0\x80\xff")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(result.stderr, b"");
    assert_eq!(result.stdout, b"\0\x80\xff|2");
}
