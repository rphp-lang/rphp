use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/file_object_read/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/file_object_read/contract.php"
            ))
            .env("RPHP_FILE_READ_CASE", name)
            .env_remove("RPHP_DISABLE_JIT");
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(
            output.stdout,
            expected[name].as_str().unwrap().as_bytes(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
macro_rules! specimen {
    ($name:ident) => {
        #[test]
        fn $name() {
            contract(stringify!($name));
        }
    };
}
specimen!(metadata);
specimen!(tell);
specimen!(bytes);
specimen!(chunks);
specimen!(lines);
specimen!(lengths);
specimen!(cache);
specimen!(csv);
specimen!(arguments);
specimen!(strict);
specimen!(reentry);
specimen!(permissions);
specimen!(cache_eof);
specimen!(overrides);
#[cfg(feature = "stream-registry")]
specimen!(wrapper_seek);
#[cfg(feature = "stream-registry")]
specimen!(wrapper_failure);
#[cfg(feature = "stream-registry")]
specimen!(wrapper_short);
#[cfg(feature = "stream-registry")]
specimen!(wrapper_empty);
#[cfg(feature = "stream-registry")]
specimen!(wrapper_reentry);
#[cfg(feature = "stream-registry")]
specimen!(wrapper);
