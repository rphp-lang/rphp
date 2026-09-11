#[test]
fn leaf_capture_preserves_all_arguments_guards_and_fallback_effects() {
    for disable_jit in [false, true] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"));
        command.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/scalar_call_capture.php"
        ));
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(output.stdout, b"capture:ok\n");
    }
}
