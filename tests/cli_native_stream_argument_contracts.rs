use std::process::Command;

fn check(name: &str, expected: &str) {
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .arg(format!(
            "{}/tests/fixtures/native_stream_args/{name}.php",
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
                    "fixtures/native_stream_args/",
                    stringify!($name),
                    ".out"
                )),
            );
        }
    };
}

specimen!(resources);
specimen!(read_bounds);
specimen!(seek_bounds);
specimen!(write_bounds);
specimen!(strict);
specimen!(callback_state);
specimen!(pure_coercions);
#[cfg(feature = "stream-context")]
specimen!(wrong_resource);
