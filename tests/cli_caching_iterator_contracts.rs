use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/caching_iterator/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/caching_iterator/contract.php"
            ))
            .env("RPHP_CACHING_ITERATOR_CASE", name)
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
specimen!(empty);
specimen!(lookahead);
specimen!(cache);
specimen!(offsets);
specimen!(flags);
specimen!(setflags);
specimen!(strings);
specimen!(order);
specimen!(throw);
specimen!(conversion);
specimen!(reference);
specimen!(cow);
specimen!(live);
specimen!(strict);
specimen!(uninitialized);
specimen!(nocache);
specimen!(lifetime);
specimen!(bytes);
specimen!(diagnostics);
specimen!(cycle);
