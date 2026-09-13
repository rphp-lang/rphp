use std::process::Command;

#[test]
fn computed_numeric_results_preserve_bits_aliases_and_retirement() {
    let expected = format!(
        concat!(
            "{}4:4\nretire\n9\n9:9\nmissing\n10:1:11:9\n12:5\n44:44:6\n44:44:12\n44:44:18\nunresolved\n12:13\n7:7:9.5:10:type\n",
            "integer:0:1;integer:0:1;integer:1:1;integer:-7:1;integer:8:1;\n",
            "integer:0:1;integer:0:1;integer:1:1;integer:-7:1;integer:8:1;\n",
            "integer:1:1;integer:1:1;integer:2:1;integer:-6:1;integer:9:1;\n",
            "integer:-7:1;integer:-7:1;integer:-6:1;integer:-14:1;integer:1:1;\n",
            "integer:8:1;integer:8:1;integer:9:1;integer:1:1;integer:16:1;\n"
        ),
        "integer:14:0.75:double:8000000000000000:1:1\n".repeat(5)
    );
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/numeric_result_forwarding.php"
            ))
            .env_remove("RPHP_DISABLE_JIT");
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(output.stdout, expected.as_bytes());
    }
}
