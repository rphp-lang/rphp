use std::process::Command;

#[test]
fn repeated_class_constants_preserve_cow_autoload_visibility_and_deprecation() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/class_constant_cache_replay/contracts.php"
        ))
        .output()
        .expect("run constant replay contracts");
    assert_eq!(output.status.code(), Some(0), "{:?}", output.stderr);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(
        output.stdout,
        include_bytes!("fixtures/class_constant_cache_replay/contracts.out")
    );
}
