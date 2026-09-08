use std::process::Command;

fn check(name: &str, expected: &str) {
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "error_reporting=32767",
        ])
        .arg(format!(
            "{}/tests/fixtures/array_object_backing/{name}.php",
            env!("CARGO_MANIFEST_DIR")
        ))
        .output()
        .expect("start isolated CLI specimen");
    assert_eq!(result.status.code(), Some(0), "{name}: {:?}", result.stderr);
    assert!(result.stderr.is_empty(), "{name}: {:?}", result.stderr);
    assert_eq!(result.stdout, expected.as_bytes(), "{name}");
}

macro_rules! specimen {
    ($name:ident) => {
        #[test]
        fn $name() {
            check(
                stringify!($name),
                include_str!(concat!(
                    "fixtures/array_object_backing/",
                    stringify!($name),
                    ".out"
                )),
            );
        }
    };
}

specimen!(basic);
specimen!(nested);
specimen!(raw_slots);
specimen!(references);
specimen!(diagnostics);
specimen!(reentrant);
specimen!(metadata);
specimen!(append_method);
specimen!(scalar_fallback);
specimen!(detached_write);
specimen!(finally_frames);
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
specimen!(directory);
