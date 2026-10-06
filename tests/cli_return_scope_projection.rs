use std::process::Command;

#[test]
fn lazy_return_scope_preserves_coercion_callbacks_and_finally_references() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/return_scope_projection/reentry.php"
        ))
        .output()
        .expect("run return scope contracts");
    assert_eq!(output.status.code(), Some(0), "{:?}", output.stderr);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(
        output.stdout,
        include_bytes!("fixtures/return_scope_projection/reentry.out")
    );
}
