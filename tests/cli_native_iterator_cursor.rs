use std::process::Command;

fn specimen(name: &str) {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/native_iterator_cursor/expected.json"
    ))
    .unwrap();
    for disabled in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/native_iterator_cursor/contract.php"
            ))
            .env("RPHP_NATIVE_ITERATOR_CURSOR_CASE", name)
            .env_remove("RPHP_DISABLE_JIT");
        if disabled {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected[name].as_str().unwrap(),
            "{name} JIT disabled={disabled}"
        );
    }
}
macro_rules! case {
    ($name:ident) => {
        #[test]
        fn $name() {
            specimen(stringify!($name));
        }
    };
}
case!(empty_operations);
case!(empty_override);
case!(infinite_callbacks);
case!(infinite_empty);
case!(infinite_composition);
case!(infinite_constructor);
case!(infinite_callback_exception);
case!(infinite_clone);
case!(multiple_flags);
case!(multiple_empty);
case!(multiple_labels);
case!(multiple_attach_errors);
case!(multiple_callbacks);
case!(multiple_mutation);
case!(multiple_generators);
case!(multiple_clone);
case!(multiple_lifetime);
case!(multiple_cycle);
case!(multiple_offsets);
case!(multiple_debug);
case!(multiple_reinitialize);
case!(metadata);
case!(multiple_live_label);
case!(multiple_live_flags);
case!(multiple_errors_resume);
case!(multiple_value_cow);
case!(multiple_native_info);
case!(multiple_info_conversion);
