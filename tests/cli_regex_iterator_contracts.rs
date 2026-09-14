use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/regex_iterator/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/regex_iterator/contract.php"
            ))
            .env("RPHP_REGEX_ITERATOR_CASE", name)
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
specimen!(initial);
specimen!(modes);
specimen!(keys);
specimen!(invert);
specimen!(mutation);
specimen!(replacement);
specimen!(preg_flags);
specimen!(arguments);
specimen!(strict);
#[test]
fn overrides() {
    contract("override");
}
specimen!(order);
specimen!(throw);
specimen!(casting);
specimen!(bytes);
specimen!(lifetime);
specimen!(empty_columns);
specimen!(raw_pattern);
specimen!(reentry);
specimen!(filter_base);
specimen!(cycle);
