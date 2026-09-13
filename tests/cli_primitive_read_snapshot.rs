use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/primitive_read/expected.json")).unwrap();
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/primitive_read/contract.php"
            ))
            .env("RPHP_PRIMITIVE_READ_CASE", name)
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
specimen!(scalars);
specimen!(aliases);
specimen!(float_bits);
specimen!(undefined);
specimen!(throw_order);
specimen!(unpack);
specimen!(owners);
specimen!(large_frame);
specimen!(retirement);
