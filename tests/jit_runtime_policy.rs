#![cfg(all(
    feature = "jit-prototype",
    feature = "vm-stats",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]

use std::path::PathBuf;
use std::process::{Command, Output};

fn ledger_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/corpus_ledger_pipeline.php")
}

fn run_with_environment(values: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
    command
        .arg(ledger_fixture())
        .env("RPHP_VM_STATS", "1")
        .env_remove("RPHP_DISABLE_JIT")
        .env_remove("RPHP_JIT_CODE_LIMIT_BYTES");
    for (name, value) in values {
        command.env(name, value);
    }
    command.output().expect("rphp subprocess should run")
}

fn output_value(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout)
        .expect("rphp stdout should be UTF-8")
        .split_once('|')
        .expect("fixture output should contain timing separator")
        .0
}

fn stderr(output: &Output) -> &str {
    std::str::from_utf8(&output.stderr).expect("rphp stderr should be UTF-8")
}

fn stat_value(output: &Output, name: &str) -> u64 {
    stderr(output)
        .lines()
        .find_map(|line| line.strip_prefix(name))
        .and_then(|value| value.strip_prefix('='))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("missing numeric stat {name}"))
}

fn native_execution_stats(output: &Output) -> &str {
    stderr(output)
        .split_once("-- native JIT executions by shape --\n")
        .expect("native execution section should be present")
        .1
        .split_once("-- rejected loops by dominant gap --")
        .expect("native execution section should have an end")
        .0
}

#[test]
fn default_jit_opt_out_and_code_budget_keep_the_same_fallback_result() {
    let native = run_with_environment(&[]);
    let disabled = run_with_environment(&[("RPHP_DISABLE_JIT", "1")]);
    let exhausted = run_with_environment(&[("RPHP_JIT_CODE_LIMIT_BYTES", "0")]);

    assert!(native.status.success());
    assert!(disabled.status.success());
    assert!(exhausted.status.success());
    assert_eq!(output_value(&native), output_value(&disabled));
    assert_eq!(output_value(&native), output_value(&exhausted));

    assert!(stderr(&native).contains("jit_runtime_enabled=1"));
    assert!(stat_value(&native, "jit_code_mapping_created_count") > 0);
    assert!(native_execution_stats(&native).contains("typed_ops_loop=1,side_exits=0"));

    assert!(stderr(&disabled).contains("jit_runtime_enabled=0"));
    assert_eq!(stat_value(&disabled, "jit_code_mapping_created_count"), 0);
    assert!(stat_value(&disabled, "jit_code_mapping_disabled_rejections") > 0);
    assert!(!native_execution_stats(&disabled).contains("typed_ops_loop="));

    assert!(stderr(&exhausted).contains("jit_runtime_enabled=1"));
    assert_eq!(stat_value(&exhausted, "jit_code_mapping_limit_bytes"), 0);
    assert_eq!(stat_value(&exhausted, "jit_code_mapping_created_count"), 0);
    assert!(stat_value(&exhausted, "jit_code_mapping_budget_rejections") > 0);
    assert!(!native_execution_stats(&exhausted).contains("typed_ops_loop="));
}
