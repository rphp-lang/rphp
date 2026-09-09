use std::process::Command;

#[test]
fn scalar_slot_admission_preserves_references_release_order_and_unwind() {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/frame_slot_admission.php"
        ))
        .output()
        .expect("run slot admission specimen");
    assert_eq!(output.status.code(), Some(0), "{:?}", output.stderr);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(output.stdout, b"7,0,drop:v0;0/10;8,1,drop:v1;1/11;9,2,drop:v2;2/12;10,3,drop:v3;3/13;return;drop:held;NULL\ndrop:unwind;finish\n");
}
