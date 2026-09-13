use std::process::Command;

#[test]
fn baseline_continuations_keep_callbacks_finally_and_source_context() {
    let source = include_str!("fixtures/baseline_continuation.php");
    let expected = concat!(
        "H:1\nC:0\nF:0\nH:2\nC:1\nF:1\nH:3\nC:2\nF:2\n",
        "G:0=4\nG:1=5\nG:tail=6\nGF\n",
        "D:11\nD:12\nE:13\nN:1\n",
        "S:2\nS:3\nR:8\nRF\nM:9\n"
    );
    for disable_jit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command.args(["-r", source.trim_start_matches("<?php\n")]);
        command.env_remove("RPHP_DISABLE_JIT");
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(output.stdout, expected.as_bytes(), "{output:?}");
    }
}
